# FORGE-675 — Document "run `cx olly ask` in the background and poll" in `skills/cx-olly/SKILL.md`

## Scope

**Docs only. Exactly one file changes: `skills/cx-olly/SKILL.md`.** No changes to `src/commands/olly/*`, `src/main.rs`, or tests.

Decision: **do not** edit `skills/cx-ai-center/SKILL.md:200` — it is a one-line "Related Skills" cross-reference (`- \`cx-olly\` — the conversational AI assistant (\`cx olly ask\`).`); adding backgrounding guidance there duplicates the cx-olly skill and widens merge surface for no benefit.

Interpretation of the ticket title ("default in cli skill file"): backgrounding is presented as the **default recommended pattern for agent-initiated `cx olly ask` calls**, not merely an option for exceptionally long ones. Synchronous/foreground stays documented for quick interactive/human use.

## Ground truth confirmed in the worktree (do not contradict it)

- `src/main.rs:743-766` — `OllyCmd::Ask` flags today: positional `message`, `--chat-id`, `--model` (default `gpt-5.2`), `--timeout` (default `900`). **No `--background`/`--async`.**
- `src/commands/olly/api.rs:203-221` — `send_message()` hardcodes `"should_block": true` and passes `timeout_seconds`. `src/api_client.rs` sets **no** client-side HTTP timeout, so the wall-clock wait is governed server-side by `timeout_seconds`.
- `src/commands/olly/api.rs:223-230` — `get_interaction(chat_id, interaction_id)` exists but is **not wired to any subcommand**. There is no `cx olly interactions get` / `cx olly status`. **Never document one.**
- `src/commands/olly/mod.rs:50-57` — progress lines `Creating new chat...` / `Sending message...` go to **stderr**; the result goes to stdout.
- `src/commands/olly/mod.rs:60-74, 351-380` — `-o json` prints a **single-element array**: `[{ chat_id, interaction_id, status, response?, interaction_mode?, model_choice? }]` → parse with `jq -r '.[0].response'`. `-o agents` emits **TOON**, not JSON, so scripted parsing of a backgrounded run must use `-o json`.
- `src/commands/olly/mod.rs:382-410` — when status is non-terminal and no assistant text exists, text output prints `No response received.`; error → `Generation encountered an error.`; stopped → `Generation was stopped.` (statuses are matched case-insensitively, so the API's `IN_PROGRESS`/`COMPLETED` and lowercase forms both occur).

## Reference pattern being ported

cx-olly PR #608 (`apps/ws-ai-mcp/src/mcp_server/skills/olly/SKILL.md`, still OPEN) adds:
1. a `### Run in background` subsection (`olly_ask(..., should_block=False)` then loop on `olly_get_interaction` until status is terminal), and
2. a status-table row change: `in_progress` → "Poll ... until status is terminal" instead of "unusual with blocking".

The CLI has neither `should_block=False` nor a get-interaction command, so the adaptation is: **the calling agent backgrounds the whole blocking `cx olly ask` process and polls the job/output file** — the process itself is the interaction handle.

## Edits to `skills/cx-olly/SKILL.md` (in order)

Re-read the file before editing — see the merge-order note below; line numbers below refer to today's `master` version (132 lines).

1. **Intro (after line 10, and after the `--agent-to-agent-mode` intro line if PR #186 has landed):** add one line, e.g. — `cx olly ask` blocks until Olly finishes and can take many minutes. **If you're an agent, launch it in the background and poll instead of blocking your turn** — see "Run in background".

2. **CLI Commands table (line 16):** in the `cx olly ask` row, keep the flag list as-is and append to Purpose: *(blocking — background it yourself for long queries)*. Do **not** invent a flag column entry.

3. **Keep the `### Timeout` subsection (lines 50-56)** but reframe it as the bound on the background job: state that `--timeout` (default 900s) is the server-side cap and that raising it is complementary to — not a substitute for — backgrounding. Point to the new subsection.

4. **Add a new `### Run in background (preferred for long queries)` subsection immediately after Timeout**, containing:
   - *Why*: the CLI always sends `should_block: true`; there is no `--background` flag and no CLI polling subcommand, so the agent must background the process itself.
   - *Preferred (agent harness)*: use your own background-execution capability (e.g. Claude Code's `Bash` tool with `run_in_background`, then poll its output) so your turn is not blocked. Explicitly tell agents **not** to sit in a long blocking `sleep`.
   - *Plain shell fallback*, keeping stdout parseable:
     ```bash
     cx olly ask "Perform root cause analysis for the outage on 2024-01-15" \
       --timeout 1800 -o json > /tmp/olly_rca.json 2> /tmp/olly_rca.log &
     OLLY_PID=$!
     # poll every ~15-30s; do other work between checks
     kill -0 "$OLLY_PID" 2>/dev/null && echo "still running"
     # when the process is gone, read the result
     jq -r '.[0].status, .[0].chat_id, .[0].response' /tmp/olly_rca.json
     ```
     Mention `nohup ... & disown` if the shell/session may go away.
   - *Parsing*: use `-o json` (single-element array; `.[0].chat_id`, `.[0].interaction_id`, `.[0].status`, `.[0].response`); `-o agents` is TOON. Redirect stderr separately because progress lines are written there.
   - *Caveat*: because the POST blocks, `chat_id` is only printed when the command finishes — you cannot follow up mid-flight; wait for the job before using `--chat-id`.
   - *Useful*: interleave other work while it runs (e.g. `cx logs` / `cx spans` / `cx alerts` queries).
   - If PR #186 has landed, include `--agent-to-agent-mode` in the snippet and repeat its "re-pass on every follow-up" rule.

5. **Add a short `### Interaction status` table** (mirrors PR #608's status-table intent, adapted to the CLI). Rows and CLI-accurate actions:
   - `completed` — response available, use `.[0].response`.
   - `in_progress` — the server hit `timeout_seconds` before finishing; `response` may be absent (text output prints `No response received.`). Re-ask the **same** `--chat-id` with a larger `--timeout` (context is preserved) and/or check `cx olly artifacts list` for partial output. There is no CLI command to fetch the interaction by id.
   - `error` — read the message, retry with a narrower/more concrete query.
   - `stopped` — cancelled/interrupted, retry if needed.

6. **Update the `### Detailed analysis with specific model` workflow example (lines 113-119)** so the deep-RCA example is the backgrounded form (`... &` + poll + `jq` read) rather than only `--timeout 1800`. Optionally add a single comment line to `### Investigate an issue` (lines 89-101) pointing at "Run in background" for deep investigations — keep it to one comment line to limit conflict with PR #186.

7. **Key Principles (lines 121-127):** add one bullet — `**Background long asks** - cx olly ask blocks (no --background flag); launch it as a background job and poll for completion instead of blocking your turn`.

8. **Frontmatter:** optionally add 1-2 trigger phrases to `description` (e.g. "run olly in background", "long-running olly investigation"). Leave `metadata.version` at `0.1.0` — version bumps are not applied consistently in this repo and PR #186 does not bump it.

Style constraints: keep the existing tone (imperative, short bash blocks), stay well under the 400-line cap enforced by `scripts/verify-skills.sh` (132 lines today, ~146 after PR #186; target ≤ ~200), and only reference real top-level commands (`verify-skills.sh` check 4 validates every `` `cx <cmd>` `` against `cx schema`).

## Ordering / merge-order dependency (important)

`FORGE-591` / PR [cx-cli#186](https://github.com/coralogix/cx-cli/pull/186) is **OPEN** and edits the *same* regions of this file (intro line, flags-table row for `cx olly ask`, the Investigate example, Key Principles). Therefore:

1. First `git fetch origin && git rebase origin/master` (or merge master) and check whether #186 has landed (`git log --oneline -- skills/cx-olly/SKILL.md`, `gh pr view 186 --repo coralogix/cx-cli --json state`).
2. If landed: build on its wording — the flags table already lists `--agent-to-agent-mode`, and examples already carry the flag, so new snippets should carry it too.
3. If not landed: prefer **additive new subsections** over rewriting the shared lines it touches, and call out the overlap in the PR description so whoever merges second resolves it deliberately.

## Risks / edge cases

- Don't document non-existent CLI surface (`--background`, `--async`, `cx olly status`, `cx olly interactions get`). The unused `get_interaction()` API method stays unused.
- `scripts/verify-skills.sh` fails on >400 lines and on unknown top-level `cx <cmd>` references — keep additions tight and use only real commands.
- Shell snippets: `kill -0`/`$!`/`jq` are POSIX-shell/Unix-specific. Add a sentence telling agents on non-POSIX shells (or with their own background tool) to use the harness capability instead of the shell snippet.
- Don't encourage tight poll loops (hammering) or long blocking sleeps — recommend ~15-30s between checks, bounded by `--timeout`.
- Keep guidance consistent with `agent_to_agent_mode` semantics (per-call, must be re-passed on follow-ups) once #186 lands.

## Verification

**Environment blocker (recorded, does not block this docs-only change):** `cargo build` fails in this sandbox — `libdbus-sys v0.2.7` build script aborts because `pkg-config` is missing (`keyring` → `sync-secret-service` backend on Linux). Fixing it needs root (`apt install pkg-config libdbus-1-dev`; `dpkg` lock is permission-denied for uid 1000) and no `x86_64-unknown-linux-musl` target is installed. So the `cx` binary could not be built or run here, and there is no `CX_API_KEY`. Before-state was therefore captured statically; see `.saga/artifacts/cli-before-state.txt`.

Checks the implementation step must run:

- `bash scripts/verify-skills.sh` — must print `PASS` for `cx-olly` and `ALL PASSED` (baseline before the change: `cx-olly PASS (14 triggers, 132 lines)`, `17/17 passed, 0 errors`). It warns `cx schema unavailable` here, which is expected without a built binary.
- CI equivalent of spec validation: `pip install "skills-ref==0.1.1" && agentskills validate skills/cx-olly` (`agentskills` is not installed in this sandbox; `.github/workflows/skills.yml` runs it on any `skills/**` change).
- Rust gates are unaffected (no `.rs` changes). If a buildable machine is available, run `/run-tests` (`cargo fmt --check`, `cargo clippy`, `cargo test`) for hygiene; otherwise rely on CI.
- Human/behavioral check of the documented snippet (needs a real tenant + `CX_API_KEY`, cannot be done here): run the backgrounded command from the new subsection and confirm (a) the shell returns immediately, (b) `/tmp/olly_rca.json` ends up containing a one-element JSON array with `chat_id`/`interaction_id`/`status`/`response`, (c) progress lines land only in the stderr file so `jq` on stdout succeeds.

**Before → after to observe:** before, the only long-query guidance in `skills/cx-olly/SKILL.md` is "increase `--timeout`" (lines 50-56 and 113-119) and an agent reading the skill blocks synchronously for up to 900s+; after, the skill's default instruction for agents is to launch `cx olly ask` as a background job, poll it, and read the JSON result from the redirect file, with an interaction-status table telling it what to do for `in_progress`/`error`/`stopped`.

**Artifacts:** `.saga/artifacts/cli-before-state.txt` already written. After the edit, write `.saga/artifacts/cli-after-state.txt` containing `git diff -- skills/cx-olly/SKILL.md` plus the post-change `scripts/verify-skills.sh` output (line count for `cx-olly`).
