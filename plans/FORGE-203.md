# FORGE-203 — Replace mypy with ty in knowledge-base

## Summary

Replace `mypy` with `ty` as the static type checker across all 14 Python packages in the `knowledge-base` monorepo, matching the Olly repo's tooling. This is a per-package change (no workspace root): update each `pyproject.toml`, each `scripts/lint.sh` that runs mypy, and fix all type errors that ty surfaces (or narrowly scope-suppress them per Olly precedent). CI structure requires no changes — `.github/workflows/lint.yaml` already shells out to per-service `scripts/lint.sh`.

## Ground-state (observed)

Explored the worktree at `knowledge-base/`. Verified:

- **14 `pyproject.toml` files exist** (matches the ticket): 11 under `apps/*`, plus `common/`, `tools/evals-kb-loader/`, `tests/`.
- **13 have `[tool.mypy]` sections** — all except `tests/pyproject.toml` (no type-checking today at all).
- **13 `scripts/lint.sh` files exist** — one per package with a mypy section. `tests/` has none.
- Of the 13 `lint.sh` files, **11 actually invoke `mypy`** — `apps/prep-keys-service/scripts/lint.sh` and `apps/writer-service/scripts/lint.sh` only run `ruff check` + `ruff format --check` (they have `[tool.mypy]` config that is never exercised). Verified by cat-ing each script.
- `apps/click-house/` is a Helm chart (no `pyproject.toml`) — not in scope.
- CI: `.github/workflows/lint.yaml` fans out via `dorny/paths-filter` change-detection and runs `/bin/sh ./scripts/lint.sh` per changed service. It does NOT generate protos before linting.
- CI: `.github/workflows/unit-tests.yaml` DOES generate protos before running tests. Two proto layout patterns exist:
  - `common/src/common/generated/**` (populated by `common/scripts/auto_generate_protos.sh`, used by ingestion/api/semantic-search/values-reader/description services)
  - Per-service `<service>/src/<pkg>/generated/**` (api-service, values-reader-service, prep-keys-service, semantic-search-service, team-populator-service)
- Only skeleton `generated/__init__.py` is committed; the actual `*_pb2.py` files are generated on-demand.
- Existing mypy configs universally use `strict = false`, `ignore_missing_imports = true`, `disable_error_code = ["import-untyped", "import-not-found", "name-defined"]` (with a few variations — see per-package notes below), and exclude `generated`/`protos`. Some packages (`prep-keys-service`, `team-populator-service`) use `ignore_errors = true` overrides for generated modules.
- `mypy-protobuf` (`protoc-gen-mypy`) is used as a `.pyi` stub generator (see `apps/api-service/scripts/generate_protos.sh:80-87`). It is not the mypy checker — it stays (out of scope, per ticket).
- `.claude/rules/backend-guide.md:39` mentions "Type Checking: mypy" — needs updating.
- `README.md:615` lists "mypy: Static type checking" — needs updating.
- `.claude/skills/run-code-checks/SKILL.md` references `just` commands that don't exist in this repo (it's copied from Olly) — leave alone; not in scope for FORGE-203.
- **`ty` config schema verified locally** with `ty 0.0.40`. Valid `[tool.ty.*]` sections: `environment`, `src`, `rules`, `terminal`, `analysis`, `overrides`. `[tool.ty.analysis].allowed-unresolved-imports` works but requires listing both the base module and its `.*` submodule glob (e.g. `["confluent_kafka", "confluent_kafka.*"]`) — a single glob alone does not suppress the base import. `[tool.ty.analysis].replace-imports-with-any` did not suppress errors in my test; prefer `allowed-unresolved-imports`.

### Run / check commands (this repo, no root justfile)

```bash
# Per-package lint (what CI runs)
cd <package>
uv sync --all-extras
bash scripts/lint.sh          # current: ruff + mypy → target: ruff + ty check

# Per-package tests
uv run pytest

# Per-package after migration (spot check)
uv run ty check
```

There is no root `justfile`. The `run-code-checks` skill's `just`-based commands don't apply here — verify per-package.

## Design decisions

1. **`[tool.ty.*]` block per package** — mirror Olly's `libs/common/pyproject.toml:85-98` pattern. Each block contains:
   ```toml
   [tool.ty.environment]
   python-version = "3.12"

   [tool.ty.src]
   include = ["src"]                        # match today's mypy `files = ["src"]`
   exclude = ["src/**/generated/**", "protos"]   # match today's mypy exclude

   [tool.ty.analysis]
   # Retain today's ignore_missing_imports behaviour ONLY for the specific modules
   # that today's mypy overrides list. No blanket suppression.
   allowed-unresolved-imports = [
       "confluent_kafka", "confluent_kafka.*",
       "<per-package modules — see below>",
   ]
   ```
2. **No blanket rule-weakening.** Do NOT set an equivalent of `strict = false` / `disable_error_code = [...]` / global `ignore_missing_imports = true` in ty. Any `[tool.ty.rules]` override must be justified per rule (see "Handling type errors" below).
3. **`ty` as dev dependency per package.** Since there is no workspace root, each of the 14 `pyproject.toml` files must add `ty` to its own `[dependency-groups].dev` (or `[project.optional-dependencies].dev` if that's the only dev group the package has). Pin to `>=0.0.55` (later than Olly's `>=0.0.20`, because 0.0.55 fixed at least one pydantic-narrowing gap called out in FORGE-1 comments; use the latest stable at implementation time if newer).
4. **Generated protobuf code — do not require it at lint time.** Match today's behavior: exclude `generated`/`protos` from ty's src scan, and add `<pkg>.generated`, `<pkg>.generated.*`, `common.generated`, `common.generated.*` (as applicable per package) to `allowed-unresolved-imports`. This preserves the current property that `.github/workflows/lint.yaml` does not need to run `generate_protos.sh` before linting. (Do NOT add proto-generation steps to the lint workflow — that expands scope and adds SSH-key/cache complexity already handled by the unit-tests workflow.)
5. **Suppression policy for real errors.** When ty flags a real type error under its defaults:
   - **Prefer fixing the code** (add annotations, `cast`, `assert isinstance`, narrow with `if x is not None`, etc.). Runtime behaviour must not change (per "Out of scope" in the ticket).
   - **Only when a fix is genuinely infeasible** (untyped 3rd-party surface, generated code shim, known ty limitation), add a narrow `# ty: ignore[<rule>]` on the specific line and note why in a code comment. This mirrors Olly's TypedDict carve-out precedent.
   - **Never** add a blanket rule disable in `[tool.ty.rules]` as a first response.
6. **Lint scripts.** Replace `uv run --all-extras mypy [args]` with `uv run ty check` in each `scripts/lint.sh`. For `prep-keys-service` and `writer-service` (which don't currently invoke mypy at all), ADD `uv run ty check` to their `scripts/lint.sh` so they participate in the same CI gate as everyone else. This is a behaviour change but keeps the repo consistent — flag in PR description.
7. **`tests/` package.** Ticket lists `tests` as one of the 14 packages that must type-check. It currently has no `[tool.mypy]` and no `scripts/lint.sh`, and is not in the CI lint matrix. Do the minimum: add a `[tool.ty.*]` block to `tests/pyproject.toml` and a `tests/scripts/lint.sh` matching the pattern. Do NOT add `tests` to `.github/workflows/lint.yaml`'s matrix — that expands scope (would need `dorny/paths-filter` and matrix entries) and the ticket says "no CI workflow structure change should be needed beyond the lint.sh script contents." Note this as a follow-up in the PR.
8. **`mypy-protobuf` stays.** The `protoc-gen-mypy` binary from `mypy-protobuf` generates `.pyi` stubs consumable by any type checker — verified in `apps/api-service/scripts/generate_protos.sh:69-88` (it's a plugin passed to `grpc_tools.protoc`, not tied to the mypy binary). Keep the dep. (Ticket explicitly OoS.)

## Per-package inventory & change list

For each package below: (a) `[tool.mypy]` state today, (b) `[dependency-groups].dev` / `[project.optional-dependencies].dev` mypy lines to remove, (c) `scripts/lint.sh` current mypy invocation, (d) modules that need to end up in `allowed-unresolved-imports`.

| # | Package | Path | Today's mypy overrides modules | lint.sh mypy invocation | Notes |
|---|---|---|---|---|---|
| 1 | common | `common/pyproject.toml` | `confluent_kafka.*`, `asyncpg.*`, `generated.*` | `uv run --all-extras mypy .` | Two mypy deps: `[project.optional-dependencies].dev` (`mypy>=1.0.0`) and `[dependency-groups].dev` (`mypy>=1.18.1`, `mypy-protobuf>=3.6.0`). Remove `mypy` from both; keep `mypy-protobuf`. |
| 2 | apps/api-service | `apps/api-service/pyproject.toml` | `pgvector.*`, `dateutil.*`, `confluent_kafka.*`; second override sets `disable_error_code = ["attr-defined"]` on `api_service.grpc.*` and `api_service.main` | `uv run --all-extras mypy` | Also add `api_service.generated`, `api_service.generated.*`, `common.generated`, `common.generated.*` to allowed-unresolved-imports. The `attr-defined` override on `api_service.grpc.*` is because generated `_pb2` classes look attribute-less to mypy — under ty this maps to `unresolved-attribute`. Prefer narrow line-level `# ty: ignore[unresolved-attribute]` (there are already ~3 `# type: ignore[attr-defined]` comments in the code). |
| 3 | apps/dashboard-service | `apps/dashboard-service/pyproject.toml` | none | `uv run --all-extras mypy` | Depends on prep-keys-service via path; imports `prep_keys_service.*`. |
| 4 | apps/description-service | `apps/description-service/pyproject.toml` | `confluent_kafka.*`, `asyncpg.*` | `uv run --all-extras mypy` | Mypy dep in BOTH `[project.optional-dependencies].dev` (`mypy>=1.0.0`) AND `[dependency-groups].dev` (`mypy>=1.18.1`). Remove both. |
| 5 | apps/embedding-service | `apps/embedding-service/pyproject.toml` | `confluent_kafka.*` | `uv run --all-extras mypy` | Same double-dep pattern as description-service. Remove both. |
| 6 | apps/enqueuer-service | `apps/enqueuer-service/pyproject.toml` | `pgvector.*`, `dateutil.*`, `confluent_kafka.*`, `generated.*` | `uv run --all-extras mypy` | |
| 7 | apps/ingestion-service | `apps/ingestion-service/pyproject.toml` | `pgvector.*`, `dateutil.*`, `confluent_kafka.*`, `generated.*` | `uv run --all-extras mypy` | Imports `common.generated.*` heavily. |
| 8 | apps/prep-keys-service | `apps/prep-keys-service/pyproject.toml` | `pgvector.*`, `dateutil.*`, `confluent_kafka.*`, `prep_keys_service.generated.*` with `ignore_errors = true`; excludes `src/prep_keys_service/generated/` | **NOT invoked** in lint.sh today (only ruff runs) | Behaviour change: adding `uv run ty check` to lint.sh will start enforcing types. Expect the biggest bucket of previously-hidden errors here. Also, current lint.sh doesn't have `set -e`-style flow error; keep the existing shape (`cd "$(dirname "$0")/.."; echo ...; uv run ruff check src tests; uv run ruff format --check src tests`) and append `echo "Running ty check..."; uv run ty check` before `echo "Linting complete!"`. |
| 9 | apps/semantic-search-service | `apps/semantic-search-service/pyproject.toml` | none (only base mypy config) | `uv run --all-extras mypy` | Imports `semantic_search_service.generated.*`. |
| 10 | apps/team-populator-service | `apps/team-populator-service/pyproject.toml` | `pgvector.*`, `dateutil.*`, `grpcio.*`, `team_populator_service.generated.*` with `ignore_errors = true` | `uv run --all-extras mypy` | |
| 11 | apps/values-reader-service | `apps/values-reader-service/pyproject.toml` | `clickhouse_connect.*`, `generated.*` | `uv run --all-extras mypy` | Note existing `# type: ignore[attr-defined]` in `src/values_reader_service/main.py:29` and `grpc/value_search_service.py:31,211` — port to `# ty: ignore[unresolved-attribute]` or remove if ty resolves via `allowed-unresolved-imports`. |
| 12 | apps/writer-service | `apps/writer-service/pyproject.toml` | `confluent_kafka.*`, `aioboto3.*`, `botocore.*` | **NOT invoked** in lint.sh today | Same shape change as prep-keys-service: append `uv run ty check` before "All lint checks passed!". Behaviour change — expect previously-hidden errors. Also port `# type: ignore[attr-defined]` at `src/writer_service/handlers/writer_handler.py:118`. |
| 13 | tools/evals-kb-loader | `tools/evals-kb-loader/pyproject.toml` | `pyarrow.*` | `uv run --all-extras mypy src/` | Standalone tool (not a uv workspace); no `common` dep. |
| 14 | tests | `tests/pyproject.toml` | (none — no mypy today) | (no lint.sh today) | New: add minimal `[tool.ty.*]` block; add `tests/scripts/lint.sh` running `uv run ruff check`, `uv run ruff format --check`, `uv run ty check`. Do NOT add to CI matrix in this PR. |

### `[tool.ty.analysis].allowed-unresolved-imports` — final set per package

Start from today's mypy overrides + `generated` module imports. For each package, ship at minimum:

- **common**: `["confluent_kafka", "confluent_kafka.*", "asyncpg", "asyncpg.*", "common.generated", "common.generated.*"]`
- **api-service**: `["pgvector", "pgvector.*", "dateutil", "dateutil.*", "confluent_kafka", "confluent_kafka.*", "api_service.generated", "api_service.generated.*", "common.generated", "common.generated.*"]`
- **dashboard-service**: `["confluent_kafka", "confluent_kafka.*", "common.generated", "common.generated.*", "prep_keys_service.generated", "prep_keys_service.generated.*"]` (verify actual imports during implementation)
- **description-service**: `["confluent_kafka", "confluent_kafka.*", "asyncpg", "asyncpg.*", "common.generated", "common.generated.*"]`
- **embedding-service**: `["confluent_kafka", "confluent_kafka.*", "common.generated", "common.generated.*"]`
- **enqueuer-service**: `["pgvector", "pgvector.*", "dateutil", "dateutil.*", "confluent_kafka", "confluent_kafka.*", "common.generated", "common.generated.*"]`
- **ingestion-service**: `["pgvector", "pgvector.*", "dateutil", "dateutil.*", "confluent_kafka", "confluent_kafka.*", "common.generated", "common.generated.*"]`
- **prep-keys-service**: `["pgvector", "pgvector.*", "dateutil", "dateutil.*", "confluent_kafka", "confluent_kafka.*", "prep_keys_service.generated", "prep_keys_service.generated.*", "common.generated", "common.generated.*"]`
- **semantic-search-service**: `["semantic_search_service.generated", "semantic_search_service.generated.*", "common.generated", "common.generated.*"]`
- **team-populator-service**: `["pgvector", "pgvector.*", "dateutil", "dateutil.*", "team_populator_service.generated", "team_populator_service.generated.*"]`
- **values-reader-service**: `["clickhouse_connect", "clickhouse_connect.*", "values_reader_service.generated", "values_reader_service.generated.*", "common.generated", "common.generated.*"]`
- **writer-service**: `["confluent_kafka", "confluent_kafka.*", "aioboto3", "aioboto3.*", "botocore", "botocore.*", "writer_service.generated", "writer_service.generated.*", "common.generated", "common.generated.*"]`
- **evals-kb-loader**: `["pyarrow", "pyarrow.*"]`
- **tests**: `[]` (start empty; add as needed)

Implementation should confirm each list by grepping imports in each `src/` (`grep -rn "^\s*from\s\|^\s*import\s"`) — the above is the seed from today's mypy config.

## Order of changes (dependencies-first)

1. **`common/`** — first, because every service depends on it (and adds `common.generated.*` to their allowed-unresolved-imports because `common.generated` is used across services). Getting `ty check` clean in `common` unlocks all downstream packages.
2. **Leaf apps that don't import from other apps** — `apps/api-service`, `apps/description-service`, `apps/embedding-service`, `apps/enqueuer-service`, `apps/ingestion-service`, `apps/semantic-search-service`, `apps/team-populator-service`, `apps/values-reader-service`, `apps/writer-service`, `apps/enqueuer-service`. In any order.
3. **`apps/prep-keys-service`** — required by dashboard-service (via `[tool.uv.sources]`).
4. **`apps/dashboard-service`** — depends on `prep-keys-service` at the path/uv-sources level and imports `prep_keys_service` symbols.
5. **`tools/evals-kb-loader`** — standalone; do any time after common.
6. **`tests/`** — last, once the packages it consumes (api-service, prep-keys-service, writer-service) type-check.

Rationale: `ty check` in package X does not follow imports into X's source dependencies' unchecked source trees — it uses installed site-packages — so strictly the ordering above is not a hard blocker. But keeping semantic ordering makes the PR easier to review one commit at a time.

## Per-package steps (repeat 14×)

For each `pyproject.toml`:

1. **Add `ty` dev dep.** Prefer `[dependency-groups].dev` (add `"ty>=0.0.55"`); if the package only has `[project.optional-dependencies].dev` (description-service, embedding-service currently have both), add there too. **Remove** `mypy>=...` line(s) from both groups. Keep `mypy-protobuf` untouched. Update `uv.lock` via `uv sync --all-extras`.
2. **Remove** the entire `[tool.mypy]` block AND every `[[tool.mypy.overrides]]` block.
3. **Add** a `[tool.ty.environment]` / `[tool.ty.src]` / `[tool.ty.analysis]` block, positioned in the same spot the mypy block was (keeps diffs readable). Use the shape from "Design decisions §1".
4. **Update** `scripts/lint.sh`: replace the `uv run --all-extras mypy [args]` line with `uv run ty check`. For prep-keys-service and writer-service (which don't have mypy today), add `uv run ty check` at the end. Drop `--all-extras` from the ty invocation (ty is in dev deps, `uv sync` already installed it — no need for `--all-extras` on the ty step; still leave the flag on the `ruff check` line for consistency).
5. **Run `uv sync --all-extras && bash scripts/lint.sh` locally.** For each ty error:
   - Fix in code if straightforward (annotate return types, `cast`, narrow with `assert`/`isinstance`, replace `Any` with concrete types, etc.).
   - If the error is on an untyped 3rd-party import that isn't in `allowed-unresolved-imports` yet, add the module to that list.
   - If the error is on generated proto attribute access, add narrow `# ty: ignore[unresolved-attribute]` (or the specific rule ty emits).
   - Track ALL suppressions added; each should have a code comment explaining why. Aggregate the suppression list into the PR description.
6. **Run `uv run pytest`** to guard against runtime regressions introduced while satisfying the type checker.

## `tests/` package specifics (item 14)

Add to `tests/pyproject.toml`:
```toml
[tool.ty.environment]
python-version = "3.12"

[tool.ty.src]
include = ["e2e"]

[tool.ty.analysis]
allowed-unresolved-imports = []
```

Add `"ty>=0.0.55"` to `tests/pyproject.toml`'s deps (it has no dev-group today — add either `[dependency-groups].dev` or as a top-level `[project.optional-dependencies].dev`).

Create `tests/scripts/lint.sh`:
```sh
#!/bin/sh
set -e
uv run --all-extras ruff check
uv run ty check
```

Do NOT touch `.github/workflows/lint.yaml` for `tests/`. Note this as a follow-up.

## Doc updates

- **`.claude/rules/backend-guide.md:39`** — change `Type Checking: mypy (non-strict mode due to asyncpg limitations)` → `Type Checking: ty (default rule set)`.
- **`README.md:615`** — change `mypy: Static type checking` → `ty: Static type checking`.
- Skim `README.md` around lines 540-580 for stale mypy references; leave `apps/*/README.md` alone unless they call out mypy explicitly.
- Don't touch `.claude/skills/run-code-checks/SKILL.md` — it references Olly's `just` commands, not this repo. Out of scope for FORGE-203.

## Risks & edge cases

1. **Previously-hidden type errors in prep-keys-service / writer-service.** Their `lint.sh` never ran mypy, so `strict = false` mypy hasn't been catching anything. Under ty defaults, both packages are effectively type-checking for the first time. Expect the largest error volume here. Budget time.
2. **Generated protobuf attribute access.** Today's mypy suppresses `attr-defined` via override / ignore comment. Under ty, this becomes `unresolved-attribute` on `*_pb2` classes. `allowed-unresolved-imports` silences the *import*, but attribute access on the resulting `Any` may still surface. Verify: if ty treats unresolved imports as `Unknown`/`Any`, attribute access should be permissive; if not, add per-line `# ty: ignore[unresolved-attribute]`.
3. **`pydantic.mypy` plugin.** Today's mypy configs use `plugins = ["pydantic.mypy"]` (mostly for BaseModel field inference and strict field-ordering). ty has native pydantic support; no config needed. Verify no regressions in Pydantic-heavy files (e.g. `common/src/common/models/**`, `apps/api-service/src/api_service/api/**`).
4. **`asyncpg` untyped surface.** `backend-guide.md` explicitly says "non-strict mode due to asyncpg limitations." asyncpg has partial typeshed stubs. Under ty, expect friction on `conn.fetch`/`conn.fetchrow` return types (rows are `Record`, columns are `Any`). Approach: keep asyncpg in `allowed-unresolved-imports` only if imports fail; if imports resolve but attribute types are `Any`, that's fine — no suppression needed.
5. **Overlap between `[project.optional-dependencies].dev` and `[dependency-groups].dev`.** Four packages have both (common, description-service, embedding-service). Removing `mypy` requires editing both lists in those files.
6. **`tests/pyproject.toml` has no dev group at all today.** All deps are in `[project].dependencies`. Add ty to a new `[dependency-groups].dev = ["ty>=0.0.55"]` block.
7. **`--all-extras` flag semantics.** Current lint scripts pass `--all-extras` to `uv run mypy`. The flag is a `uv` flag (installs all extras before running the wrapped command), so it's meaningful even for ty. But if ty is in `[dependency-groups].dev`, `uv sync --all-extras` in the CI step already installs it, and `uv run ty check` doesn't need `--all-extras` on the run command. Keep `--all-extras` on the ruff line (for consistency with today) and drop it on the ty line (matches Olly's `uv run ty check` pattern per ticket).
8. **`ty` version choice.** Available in dev env: 0.0.40. Olly workspace root pins `>=0.0.20`. FORGE-1 comments say 0.0.55 fixed pydantic narrowing. Pin `>=0.0.55` in this repo — safer default, may be revised to latest stable at implementation time.
9. **PR size.** 14 pyproject changes + 13 lint.sh changes + N code fixes + doc updates. Consider splitting into 3 PRs if the diff gets unwieldy: (1) common + tests + evals-kb-loader; (2) all apps that already ran mypy; (3) prep-keys-service + writer-service (largest error volume). Default to one PR unless review is blocked.
10. **Concurrency in CI.** Each service lints in parallel via matrix; the change is per-service, so partial adoption works — a failing service doesn't block others. Fine.

## Verification (before / after)

- **Before (baseline):**
  - `cd common && uv sync --all-extras && bash scripts/lint.sh` → passes today (ruff + mypy non-strict).
  - Repeat for each of the 12 other services with a `lint.sh`. All should pass or already-be-broken (we don't fix pre-existing brokenness in this PR — record any before-state failures).
  - `cd common && uv run pytest` → note pass/fail as baseline.
  - Capture `mypy` stdout as an artifact per package (`common/mypy-before.txt`, etc.) — useful for diffing against ty output.

- **After (per package, iterate until green):**
  - `uv sync --all-extras && bash scripts/lint.sh` → passes (ruff + ty check).
  - `uv run pytest` → same test count and results as before.
  - Capture `ty check` output as artifact (`<pkg>/ty-after.txt`) and the diff of remaining `# ty: ignore[...]` suppressions.

- **CI verification on PR:**
  - `.github/workflows/lint.yaml` matrix: every changed package's Lint step must be green.
  - `.github/workflows/unit-tests.yaml` matrix: every changed package's tests must still be green.
  - Since this PR touches every `pyproject.toml`, the paths-filter will trip every matrix entry — every service will be linted and tested. That's the desired coverage.

- **Grep gates (final sanity):**
  - `grep -rn "mypy" pyproject.toml common/pyproject.toml apps/*/pyproject.toml tools/*/pyproject.toml tests/pyproject.toml` — the only remaining hits should be `mypy-protobuf` (unchanged) and comment lines. No `[tool.mypy]`, no `mypy>=...` dev dep.
  - `grep -rn "mypy" apps/*/scripts/lint.sh common/scripts/lint.sh tools/*/scripts/lint.sh tests/scripts/lint.sh` — no matches.
  - `grep -rn "uv run.*mypy" .github/` — no matches (there shouldn't be any today either).
  - `grep -rn "# type: ignore" apps/ common/` — audit each remaining occurrence: does it still apply under ty? If yes, replace with `# ty: ignore[<rule>]`. If not, remove.

## Out of scope (per ticket, restated)

- Changing OpenAI-Agents-SDK TypedDict carve-outs from Olly — this repo doesn't use that SDK.
- Runtime/behavioural changes beyond satisfying the type checker.
- `mypy-protobuf` removal — it's a stub generator, not the mypy checker.
- Adding `tests/` to the CI lint matrix (deferred; note as follow-up).
- Refactoring `.claude/skills/run-code-checks/SKILL.md` — that file references Olly's justfile and doesn't match this repo's actual workflow; touching it is a separate cleanup.
