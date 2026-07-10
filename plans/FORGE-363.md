# FORGE-363 — Saga chore: clear up redundant comments

## Goal (restated to bound the work)

Sweep every `.py` file under `src/` and `tests/` and, for each comment (inline, standalone, decorative divider, or docstring), apply the reporter's two-part keep/remove test:

1. If the information is inferable from the code alone → **remove**.
2. If the adjacent code would look weird / non-obvious without the comment (comment supplies context/rationale the code can't express on its own) → **keep** (else remove).

The change is **comment-only** — no logic, control-flow, or behavior changes. The full test suite must remain green.

## Baseline observed (before-state)

Ran on commit `86d83df` on branch `saga/forge-363-saga-chore-clear-up-redundant-comments`:

- `find src -name '*.py'` → 73 files, ~12,115 lines
- `find tests -name '*.py'` → 67 files, ~23,489 lines
- standalone comment lines (`^\s*#`): 1,808 total (627 in `src/`, 1,181 in `tests/`)
- inline trailing comments (`\S+.*  # `): 441 total (135 in `src/`, 306 in `tests/`)
- decorative `# ---` section-divider lines: **483**
- step-number narration lines (`# 1.`, `# 2.`, `# Step N`): 14 in `src/`
- lint/type checks pass cleanly (one pre-existing invalid-`# noqa` warning at `tests/test_linear_client.py:134` — out of scope, do not touch)
- full pytest run: `891 passed, 18 warnings in 49.67s`

Baseline notes are captured in `.saga/artifacts/cli_baseline.txt`.

## Categories to remove (mechanically identifiable)

These are the low-value classes the ticket explicitly names or that fail the two-part test unambiguously. Removing them is safe by construction.

1. **Decorative `# ---` block/section dividers.** Long dashed lines that just visually break a file, e.g.
   - `src/saga/orchestrator/loop.py:111-113,290-292,336-338,656-658` (`# ---------------------`)
   - `src/saga/orchestrator/sessions.py:81-83,215-217,249-251`
   - `src/saga/services/linear/task_state_store.py:310-312` (also the `# Public API` divider)
   - `src/saga/services/slack/format.py:20-22,80-82,139-141,254-256`
   - `src/saga/services/session_transcript.py:39-41,188-190,292-294`
   - `src/saga/schemas/state.py:262` (`# --- typed accessors ...`)
   - `tests/test_pre_assessor.py`, `tests/test_mcp_open_pr.py`, `tests/test_github_client.py`, `tests/test_orchestrator_pr_review.py`, `tests/test_flow_generic.py`, `tests/test_terminal_aggregate.py`, `tests/test_step.py`, `tests/test_agent_session.py` (plus every other file that contains the `# ---------` pattern — the initial scan showed 483 such lines across ~30 files).
2. **Numbered step-narration inside a single function** where each numbered step is one obvious call and the number is decorative, e.g.
   - `src/saga/orchestrator/loop.py:504-565` (`# 1. Abort any in-flight agent.`, `# 2. Close every open PR ...`, `# 3. Post terminal aggregate ...`, `# 4. Post a summary comment ...`, `# 5. Close the agent session.`, `# 6. Persist final state: ...`). Some carry rationale after the digit (`# 2. Close every open PR on GitHub (best-effort; a failure must not block state cleanup)` — keep the rationale, strip the numbering); the digits themselves are decoration.
   - `src/saga/orchestrator/loop.py:581-604` (mirror block in `_complete_cleanup`).
   - `src/saga/services/plan_artifact.py:51,77,106,110` (`# 1. Push the plan file ...`, `# 2. Upload the plan ...`).
   - `src/saga/orchestrator/steps/triage/__init__.py:155,172,205,227` (`# --- Step 2: no result → needs-human ---` etc. — the surrounding code names its own behavior via `needs_human(...)` calls).
   - `src/saga/orchestrator/steps/terminal/aggregate.py:278-413` — the ten `# --- N. <Section> ---` markers inside `_summary_comment`. The section names duplicate the string literals that follow (`"**Lookback**: ..."`, `"**Triage**: ..."`); the numbers are pure decoration. Remove.
3. **Comments that just restate the immediately-adjacent code.** Examples the description calls out:
   - `tests/test_github_client.py:491,498,564,573,620,638,647,693` — narration of the git fixture (`# Base commit`, `# Two local commits`, `# Simulate upstream progress: second clone adds a commit and pushes`, `# In local: add a commit, then fetch and merge origin/main (clean merge)`, `# Verify the merge commit was created (has 2 parents)`, `# Upstream modifies file.txt`, `# Local modifies file.txt differently → conflict on merge`, `# The merge commit's file.txt blob must be the resolved content`). The git calls themselves say all this; drop.
   - Anywhere a comment paraphrases the very next line (e.g. `# Close the agent session.` immediately above `await self.session_mgr.close(task_id)`).
4. **Multi-paragraph docstrings that only re-narrate the code.** Tighten to a single line unless the extra text carries rationale, invariants, or gotchas. Do NOT delete the whole docstring — keep the leading one-line summary, drop the paragraphs that just walk through the function body.
5. **Trailing (inline) comments that duplicate the name being assigned or the field being defined.** Careful pass: many trailing comments in `state.py` and `loop.py` DO carry rationale (e.g. `_ABANDONED_STATE_TYPES = frozenset({"completed", "canceled", "backlog"})  # State types where a human has clearly moved the ticket away — don't add Needs-Human.`). Only drop the ones that don't (e.g. field-annotation echoes like `attempts: int = 0  # count` or the `was …` legacy-rename hints in `TaskState.branch_name / notifier / prs` — the "was" bits are stale and self-evident from the field name today).

## Categories to KEEP (must survive the pass)

The description calls out several classes of comments that carry non-obvious rationale — the pass MUST preserve them. Concrete examples:

- **`src/saga/orchestrator/loop.py:65-71`** — the five-line rationale for reusing one Linear app-actor token across three consumers. Not inferable from code; keep as-is.
- **`src/saga/orchestrator/loop.py:97-98`** — the `RunnerDeps.model_construct(...)` "isinstance-validating them would reject the fakes the test suite injects" note. Keep.
- **`src/saga/orchestrator/loop.py:143`** — `# Map pending tasks back to task IDs for a useful log message.` This one is borderline — the comprehension below is short but the *reason* it exists (map back for the log) is not obvious. Keep.
- **`src/saga/orchestrator/loop.py:203-204`** — `list_tasks() only returns active-column tickets, so canceled tickets vanish silently.` — non-obvious invariant; keep.
- **`src/saga/orchestrator/loop.py:214-217`** — `# One task's failure must never kill the loop: an unguarded raise here propagates ... freezing all orchestration while the process stays alive`. Keep (rationale for the surrounding `try/except`).
- **`src/saga/orchestrator/loop.py:229`** — `# Bound the state store's read-your-writes cache to live tasks (it never evicts itself).` Keep.
- **`src/saga/orchestrator/loop.py:317-325`** — the multi-line rationale block above the state update in `consume_thread_reply` (labels stay, stage reset, retry-budget reset, `step_records` survival). Keep.
- **`src/saga/orchestrator/loop.py:350-351`** — `# Drop the label in our in-memory snapshot before the await so a duplicate Approve ... is a no-op.` Keep.
- **`src/saga/orchestrator/steps/terminal/aggregate.py:33-35`** — `Canonical step spine, mirrored from registry.build_steps. The terminal aggregator runs outside the registry, so we hard-code the names here. Update if build_steps changes.` Keep — the ticket description flags this exact block as an example of what to preserve.
- **`src/saga/services/claude/mcp_tools.py:245-247`** — the scope-guard rationale (`stops a drifting agent from opening a PR in an unrelated repo`, falls back to all configured repos when triage recorded none). Keep.
- **`src/saga/services/claude/mcp_tools.py:269-273`** — the FORGE-5 base/head-derivation regression rationale. Keep.
- **`src/saga/services/linear/tracker.py:201-203`** — the `FileUpload` nested-`uploadFile` invariant (references FORGE-21). Keep.
- **`src/saga/services/linear/tracker.py:233-234`** — the "emoji prefixes are automated noise for the pre-step assessor" rationale. Keep.
- **`src/saga/services/linear/task_state_store.py:1-21`** — the module docstring explaining the write-through cache. Keep.
- **`src/saga/services/linear/client.py:150`** — `# Kept async-shaped so tests can monkeypatch GraphQlClient.execute without helper churn.` Keep.
- **`src/saga/schemas/state.py:118-119`** — the "one shape replaces the former StepOutcomeRecord ..." rationale on `StepRecord`. Keep.
- **`src/saga/schemas/state.py:308-315`** — the `_migrate_legacy` docstring. Keep.
- **`src/saga/orchestrator/steps/implementation/__init__.py:284-286`** — `# Never implement without an approved plan. ... a fresh session improvises and can latch onto unrelated work in another checked-out repo`. Keep.
- **`src/saga/orchestrator/steps/implementation/__init__.py:297-301`** — the "CLI forbids switching to bypassPermissions" comment. Keep.
- **`src/saga/orchestrator/steps/implementation/__init__.py:329-333`** — the stale-`session_id` retry rationale. Keep.
- **`src/saga/orchestrator/steps/implementation/__init__.py:394-401`** — the "flag for a human HERE rather than advancing into pr_review" rationale. Keep.
- Any comment referencing a Linear ticket ID (`FORGE-*`, `SNIR-*`) — those anchor a decision to a historical incident and are load-bearing.

When in doubt, **the ticket says "remove"** — but preservation of the above list is non-negotiable (they are the explicit success criteria).

## Order of changes

Do the sweep in four passes, checked in as one focused change per pass. This bounds review and lets us re-verify after each batch.

1. **Pass A — decorative dividers.** Delete all `# ---` block-comment separator lines (both the top and bottom of paired banners), the numbered-section markers in `terminal/aggregate.py:_summary_comment`, and the `# --- Step N: ... ---` markers in `triage/__init__.py:post_step`. This is the widest, most mechanical batch. After the removals, blank lines around former dividers may need collapsing (double blank → single) so `ruff format` stays a no-op; verify with `just lint-fix`.
2. **Pass B — restate-the-code comments in `src/`.** File-by-file walk through `src/**/*.py`; delete comments that repeat the next line's obvious meaning, and tighten multi-paragraph docstrings to a single-line summary where the paragraphs only re-narrate the code. Preserve every entry in the "KEEP" list above verbatim.
3. **Pass C — restate-the-code comments in `tests/`.** Same file-by-file walk through `tests/**/*.py`. Test files are the biggest source of narration — expect the largest volume of removals here. Focus areas by comment density: `tests/test_orchestrator_pr_review.py`, `tests/test_triage_step.py`, `tests/test_flow_generic.py`, `tests/test_technical_plan.py`, `tests/test_implementation_step.py`, `tests/test_github_client.py` (the git-fixture narration explicitly named in the description), `tests/test_terminal_aggregate.py`. Test docstrings that name what the test asserts (e.g. `"""A PR against a repo the ticket wasn't triaged for is refused; no PR is created."""`) can stay — they double as pytest -v output labels.
4. **Pass D — inline trailing comments.** Delete trailing echo comments (e.g. `x = 5  # five`, `was `foo` legacy-rename hints on renamed fields); keep every trailing comment that supplies a why/invariant, and NEVER touch a trailing `# type: ignore[...]`, `# noqa: ...`, `# ty: ignore[...]`, or `# fmt: off / on` directive.

Between passes, run `uv run ruff format` to normalise blank lines and `uv run ruff check` + `uv run pytest -q` to verify no regressions. Because we are only editing comments, ruff/ty/pytest are the ground truth for "nothing else moved".

## Files known to be comment-heavy and thus in scope for a hands-on review

From `grep -c '^\s*#'` sorted descending (top 20; every file with ≥1 comment is in scope, but these deserve the most attention):

- `tests/test_orchestrator_pr_review.py` (134)
- `tests/test_triage_step.py` (126)
- `tests/test_flow_generic.py` (86)
- `src/saga/orchestrator/loop.py` (85)
- `tests/test_technical_plan.py` (68)
- `tests/test_implementation_step.py` (58)
- `src/saga/orchestrator/steps/implementation/__init__.py` (51)
- `tests/test_terminal_aggregate.py` (46)
- `src/saga/orchestrator/steps/review/pr_monitor.py` (45)
- `tests/test_notify_slack_outcome.py` (43)
- `tests/test_orchestrator_session_lifecycle.py` (42)
- `src/saga/orchestrator/sessions.py` (38)
- `tests/test_task_state_repo.py` (35)
- `src/saga/orchestrator/steps/triage/__init__.py` (34)
- `tests/test_cancel_cleanup.py` (33)
- `src/saga/orchestrator/steps/step.py` (33)
- `tests/test_workspace.py` (32)
- `tests/test_product_definition.py` (32)
- `tests/test_task_state_store.py` (31)
- `tests/test_step_pre_step.py` (29)

## Edge cases and risks

- **Directive comments (`# type: ignore`, `# ty: ignore`, `# noqa`, `# fmt: off/on`) are load-bearing.** Removing one flips a passing lint/type-check into a failure. There are ~110 of these across the repo (`grep -n 'type: ignore\|noqa\|fmt: off\|fmt: on'`). Never delete or reformat them; if the trailing text on the same line is a comment *and* a directive, leave the directive intact.
- **Ruff/ty encoding cookies and shebangs.** None present in this repo (verified: no `#!` or coding-declaration at line 1 of any `.py`), so no false positives from a mechanical `#`-line pass — but keep the two-part test as the actual filter, not a regex.
- **Blank-line churn after divider removal.** Removing `# ---` banner pairs can leave three consecutive blank lines. Let `ruff format` collapse them; it is already a passing constraint. `just lint-fix` handles this.
- **Docstrings inside pydantic models are inert but readable.** The style guide says "no multi-paragraph docstrings"; tighten them, don't purge them wholesale — pydantic-model docstrings often carry the invariant that ties the model to its consumer.
- **Test docstrings double as `pytest -v` labels.** Keep the one-line summary; drop only paragraphs that narrate the body of the test.
- **Comments that reference `FORGE-*` / `SNIR-*` ticket IDs** anchor a decision to a historical incident — always keep these regardless of length. Same for comments that reference `docs/step-engine-plan.md §N` or similar cross-doc anchors: they are the load-bearing link between code and design docs.
- **Do not fix the pre-existing invalid `# noqa` warning** at `tests/test_linear_client.py:134` (`# noqa: unreachable`) — it is out of scope. Leave it untouched so the diff review can confirm the pass changed nothing outside comments.
- **Do not touch prompt markdown, config YAML, or docs** — the scope is `.py` files under `src/` and `tests/` only.

## Verification (how to know it's correct)

After each pass:

```bash
uv run ruff check
uv run ruff format --check   # or ruff format then re-check
uv run ty check
uv run pytest -q
```

The final gate before opening the PR:

```bash
just lint-fix   # in the saga worktree
just lint
just test       # full suite, no -k filter
```

All four expected outcomes:

- `ruff check` — clean (the one pre-existing invalid-noqa warning may still print; it must not become an error we introduced).
- `ruff format --check` — 140 files already formatted.
- `ty check` — All checks passed!
- `pytest -q` — 891 passed (or higher if any parametrised expansion happens; no failures).

**Diff-level acceptance check:**

```bash
git diff -U0 origin/main -- 'src/**/*.py' 'tests/**/*.py' | grep -E '^[-+][^-+]' | grep -v -E '^[-+]\s*(#|""")' | grep -v -E '^[-+]\s*$' | head
```

Should produce **no output** — meaning every changed line is either a comment, a docstring boundary, or blank. If any line survives that filter, it is a stray edit and must be reverted before commit.

Additionally, a reviewer spot-check on the sample the ticket names:

- `src/saga/orchestrator/loop.py:65-71` (token-sharing rationale) — must be present in the after-file.
- `src/saga/orchestrator/steps/terminal/aggregate.py:33-35` (hard-coded spine warning) — must be present.
- `src/saga/orchestrator/steps/terminal/aggregate.py:300,324,377,396` — the four `# --- N. <section> ---` decorative markers must be gone.
- `tests/test_github_client.py:491,498,564,573,620,638,647,693` — the git-fixture narration comments must be gone.

Record the after-state as `.saga/artifacts/cli_after.txt` (same fields as the baseline: comment counts, lint outcome, test outcome) so the difference between before and after is auditable.

## Not in scope

- Any change to logic, control flow, function signatures, or test assertions.
- Editing the prompt `.md` files under `src/saga/orchestrator/steps/**` — those are prompts, not code, and the ticket's Definition of Done constrains the pass to `.py` files.
- Editing config, YAML, `docs/**`, `README.md`, or `.claude/**` files.
- Reformatting or refactoring untouched code near a removed comment.
- Repos other than `saga` (the ticket's Out-of-scope list is explicit).
