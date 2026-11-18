---
name: pr-review
description: Use this skill when the user asks to "review PR", "review this PR", "review PR #123", "pr review", "code review for PR", "review my branch", "review the current branch", "check PR scope", "does this PR touch unrelated files", "test the PR", "drive the PR", "smoke test this PR", "verify the PR's testing section", "is this PR safe to merge", "what did this PR change in the schema", or wants a full review of a cx-cli pull request that reads the diff, checks scope, builds the branch, runs the changed commands in read-only mode, and reports a verdict. Also used by the CI review workflow.
version: 0.1.0
metadata:
  internal: true
---

# PR Review

Review a cx-cli pull request the way the core team would, then prove it by
building the branch and running the changed commands. The same skill runs on
a developer machine and in CI, so a local run shows exactly what the bot
would post.

The review has six parts. Do them in order. Do not skip a part; if one
cannot run, say so under **Not verified** in the report.

| Part | Question it answers |
|------|---------------------|
| 1. Gather | What does the PR claim to do, and what does CI already say? |
| 2. Scope | Does it touch only the files its purpose needs? |
| 3. Static review | Is the code correct and does it follow cx conventions? |
| 4. Build and drive | Does the binary work when a user runs the changed commands? |
| 5. Reproduce testing | Do the commands the author says they ran actually pass? |
| 6. Report | One verdict, findings by severity, evidence for every claim. |

## Inputs

`/pr-review` accepts one of:

- a PR number: `/pr-review 292`
- a branch name: `/pr-review feat/cx-datasets-list-rest`
- nothing: review the current branch against `master`
- `--profile <name>`: the cx profile to drive commands against (same as
  `CX_REVIEW_PROFILE`). Without it the skill asks before running anything.
- `--comment`: also post the report to the PR (used by CI; ask first when local)
- `--no-drive`: skip part 4 and 5 (for docs-only or dependency-only PRs)

## Safety rules (apply to every part)

- **Read-only, always.** Export `CX_READ_ONLY=true` before any `cx` call. cx
  then refuses create, update, delete, enable, disable, and set (`src/safety.rs`,
  `enforce_read_only`). Never pass `--yes`. Never run a write verb even if
  read-only mode seems to allow it; report that as a finding against this skill.
- **Pick the profile deliberately.** Use `--profile <name>` or
  `CX_REVIEW_PROFILE` if given. Otherwise ask the user which profile to drive
  against before running anything. Never
  drive against a production team without the user naming it.
- **Never print secrets.** Redact anything that looks like a token (`cxtp_`,
  `cxup_`, `Bearer `, 32+ hex or base64 chars). Check output before pasting it
  into the report.
- **Never touch the user's working tree.** All checkouts and builds happen in
  worktrees under the scratchpad directory (see part 4). Remove them at the end.
- **Bound the run.** At most 12 driven commands, each with small limits
  (`--limit 5`, `--since now-15m`). Stop driving after 10 minutes and report
  what was not covered.

## Part 1: Gather

Run these first. Keep the outputs; the report quotes them.

```bash
gh pr view <n> --json number,title,body,author,baseRefName,headRefName,isDraft,files,commits,labels
gh pr checks <n>                      # what CI already verified
gh pr diff <n>                        # the diff itself
git fetch origin pull/<n>/head:refs/pr-review/<n>
.github/scripts/pr-footprint.sh origin/master refs/pr-review/<n>
```

For a branch instead of a PR number, use the branch ref in place of
`refs/pr-review/<n>` and skip the `gh pr` calls.

From the PR body, check the eight template sections exist: Context, Linked
Issues, Design, Key Decisions, Changes, Testing, Risks & Rollout, Out of Scope.
A missing or empty section is a FIX SOON finding. A missing Testing section
means part 5 cannot run; say so.

Write one sentence: **"This PR is about X."** Derive X from the title, Context,
Changes, the linked ticket, and the branch name. Everything in part 2 is judged
against this sentence.

Do not repeat what CI already reports. If `gh pr checks` shows fmt, clippy, or
tests red, note it in one line and move on. If they are green, do not re-run
them in part 4 except for the changed command's own tests.

The **E2E** check runs the full `tests/e2e` suite, writes included, against the
writable test team on every same-repo PR that touches code (`.github/workflows/e2e.yml`).
Read its result on the head commit and quote it in the report:

- red: BLOCKER, with the failing test names from the run log
  (`gh run view <id> --log-failed | grep -E '^test .* FAILED|panicked'`).
- skipped because the path filter saw no code change: fine for docs-only PRs;
  a FIX SOON if the PR does change `src/`.
- pending: say so under **Not verified** and do not guess.

## Part 2: Scope

Input: the footprint table from `pr-footprint.sh`. Each row is `yes`, `judge`,
or `sensitive`.

- `yes` rows are in the expected footprint. No action.
- For every `judge` and `sensitive` row, ask: **does the PR text explain why
  this file had to change for X?** Read the relevant hunk; a one-line edit that
  the Design section mentions is explained. A 40-line refactor nobody mentions
  is not.
- `generated` rows (synced copies under `skills/*/references/`) are explained
  when the matching `skills/shared/` file changed.
- `ws-only` rows are never justified by the feature. They are CLEANUP.
- `tests-shrunk` rows need an explicit reason in the PR text. Without one they
  are FIX SOON.
- `version-bump` rows are BLOCKER unless the PR is a release PR whose title says
  so. Releases ship separately from features.
- Security notes make a row `sensitive` on any path. Read every matching line.
  - `secret-like`: BLOCKER, even when explained, unless the value is clearly a
    fake inside a test. A real key in a public repo is leaked the moment it is
    pushed; say "rotate the key", not only "remove the line".
  - `pipe-to-shell`: BLOCKER unless the URL is a Coralogix-owned host and the
    PR says why. The official installer is not flagged.
  - `safety-bypass`: BLOCKER unless explained. A shipped skill that tells an
    agent to pass `--yes` skips the confirmation for risky commands.
  - `new-crate`: name the crate and who publishes it. BLOCKER unless Key
    Decisions explains it; `cargo deny` checks licenses, not intent.

Severity for an **unexplained** row:

| Row | Severity | Why |
|-----|----------|-----|
| `sensitive`: domain `security` (credentials, auth, endpoints, update and install hints, `build.rs`, `.cargo/`, the e2e harness and sweeper, `.claude/`, `.codex/`, `AGENTS.md`, `scripts/`, `.github/`, `CODEOWNERS`, `deny.toml`, release files) or any security note | BLOCKER | Security, governance, or release surface. The full list is `SECURITY_RE` and `SENSITIVE_RE` in `pr-footprint.sh` |
| `judge` in another command's directory, `skills/shared/`, `core`, or a new dependency in `Cargo.toml` | FIX SOON | Expands blast radius beyond X |
| `judge` with domain `other` | FIX SOON | A file the classifier does not know. Say what it does; if it handles keys, network, or process execution, treat it as `sensitive` and add it to `SECURITY_RE` in a separate PR |
| `judge` docs edits related to X | none | Explain in the report, no finding |
| `ws-only`, comment-only, rename-only hunks outside the footprint | CLEANUP | Noise for reviewers and `git blame` |

Exception: a `CODEOWNERS` row that only adds the new command's directories is
explained by a new-command PR.

Remedy text for every scope finding: "Move this to a separate PR."

## Part 3: Static review

Read the full file for every changed hunk, not just the diff. Reviewers who
read only diffs miss callers.

Check correctness first. For each finding you must be able to write a concrete
failure: inputs or state, then the wrong output or crash. If you cannot, it is
not a BLOCKER.

Then check cx conventions. These come from `CLAUDE.md`, `CONTRIBUTING.md`, and
`contributing/adding-a-command.md`:

- **REST response types:** `#[serde(rename_all = "camelCase")]`, `#[serde(default)]`
  on every `Vec`, `Option<T>` for fields that may be absent. Deserialization
  tests exist for happy path and for empty or missing fields.
- **Client borrowing:** the API struct borrows `&CxClient`; it does not own it.
- **Output discipline:** status messages use `eprintln!`; stdout carries data
  only. All three formats (text, json, toon) are handled. Multi-profile uses
  `include_profile = targets.len() > 1`.
- **Write verbs are registered.** Any new create/update/delete/enable/disable/
  set/replace subcommand must be recognised by `is_write_verb` in
  `src/safety.rs`, or read-only mode and risky-command confirmation silently
  skip it. This is a BLOCKER when missing.
- **No secrets in output or errors.** Grep the diff for `api_key`, `token`,
  `Authorization` near `println!`, `eprintln!`, `format!`, or `anyhow!`.
- **Three test layers** for a new or changed command: unit tests in `api.rs`,
  integration tests in `tests/<cmd>/main.rs` with wiremock, e2e in
  `tests/e2e/<cmd>.rs` or `tests/e2e/<cmd>/mod.rs` declared in `tests/e2e.rs`.
  A missing unit or integration layer is FIX SOON; a missing deserialization
  test for a REST command is BLOCKER.
- **E2E must change with the command.** Every PR that changes what a command
  sends to or shows from the Coralogix API must add or update an e2e test for
  that behaviour in the same PR. This is a BLOCKER when missing, not FIX SOON:
  the suite is the first gate of the auto-SDLC flow and runs on every PR.
  - Mechanical check: the footprint summary prints `e2e updated for:` and
    `e2e NOT updated for:` per touched command. The e2e file name can differ
    from the command directory (`iam api-keys` lives in `tests/e2e/api_keys.rs`,
    `usage` in `data_usage.rs`, `archive` in `data_archive.rs`), so when the
    script says NOT updated, grep `tests/e2e` for the subcommand before you
    report it: `git grep -n '"<sub>"' refs/pr-review/<n> -- tests/e2e`.
  - What "updated" means: a new subcommand or flag gets a test that runs it;
    a changed request path, body, or response shape gets an assertion that
    would have failed before the fix. A diff that only touches the e2e file
    without exercising the changed behaviour does not count.
  - A new or changed **write** verb needs a lifecycle test on the harness
    (`tests/e2e/harness.rs`): `require_write_creds`, `unique_name`, a `Cleanup`
    guard, then create, get asserts the name, list contains it, update asserts
    the change, delete, get fails. Singletons (retention, archive targets, TCO
    settings) are round-tripped: read, write the same value, read again.
  - Exempt: local-only commands (`profiles`, `init`, `skills`, `cleanup`,
    `completions`, `schema`) need integration tests instead; docs, skills,
    deps, and dev-tooling PRs; and the commands the suite deliberately skips,
    each marked in its test with `[e2e] not covered: <reason>`. A PR that
    changes one of those must keep or update that marker, and the PR text
    must say why e2e still cannot cover it.
  - The Testing section claiming "e2e passed" is not evidence. The CI E2E
    check on the head commit is.
- **Skill shipped and synced.** A new command has a skill under `skills/`, a
  row in `skills/README.md` and in the `CLAUDE.md` coverage table, and
  `bash scripts/verify-skills.sh` passes.
- **HTTP only.** No gRPC. No new network crate without a reason in Key Decisions.

Do not report formatting, naming taste, or anything `cargo clippy` would catch.
CI owns those.

## Part 4: Build and drive

Work in worktrees so the user's checkout is untouched. Share the repo's
`target/` so the second build is incremental (about 20 seconds warm).

```bash
S="<scratchpad>/pr-review-<n>"; mkdir -p "$S"
git worktree add -q "$S/head" refs/pr-review/<n>
git worktree add -q "$S/base" origin/master
export CARGO_TARGET_DIR="$(git rev-parse --show-toplevel)/target"
(cd "$S/head" && cargo build --locked -q) && mkdir -p "$S/hb" && cp "$CARGO_TARGET_DIR/debug/cx" "$S/hb/cx"
(cd "$S/base" && cargo build --locked -q) && mkdir -p "$S/bb" && cp "$CARGO_TARGET_DIR/debug/cx" "$S/bb/cx"
export CX_READ_ONLY=true CX_PROFILE="${CX_REVIEW_PROFILE:?ask the user}"
```

A head build failure is a BLOCKER; stop here and report it.

**Schema diff.** Agents consume `cx schema`, so interface changes matter.

```bash
diff <("$S/bb/cx" schema | jq -S .) <("$S/hb/cx" schema | jq -S .)
```

Any removed or renamed command, subcommand, or flag is FIX SOON unless the PR
text calls it out as intentional. Additions are expected; list them in the
report so the reader sees the interface delta.

**Drive the changed commands.** For each command in `commands touched` from the
footprint output, using `$S/hb/cx`:

1. `cx <cmd> --help` and `cx <cmd> <sub> --help` for each new subcommand.
2. Every **read** subcommand in all three outputs: `-o text`, `-o json`, `-o toon`.
   Check json parses with `jq .`, text has a header row, toon is non-empty.
3. One invalid input per read subcommand (bad ID, bad flag value, bad time).
   The error must name the problem and must not contain a token.
4. If two profiles are configured, run one read subcommand with
   `-p <a> -p <b>` and check the Profile column appears.
5. `cargo test --locked <cmd>` inside `$S/head`.
6. If the PR says a write verb was added, run it **without** credentials to see
   the read-only refusal message. Do not run it for real.

For every command record what you expected, whether the result matched
(🟢 PASS or 🔴 FAIL), and one line of output or the error. An invalid input
that produces a clear error is a 🟢 PASS. This table goes into the report verbatim,
with a "Checks: N passed, M failed" line above it.

Skip this part when `commands touched: none` and the diff has no `src/` files,
or when the user passed `--no-drive`. Say so in the report.

## Part 5: Reproduce the Testing section

Take every command the author listed under **Testing** in the PR body. Run each
one as written, inside `$S/head`, with the same read-only environment.

- A listed command that fails is a BLOCKER: the PR claims verification it does
  not have.
- A listed command that cannot run here (needs a write, needs data the test
  team lacks) goes under **Not verified** with the reason.
- A Testing section that lists only `cargo test` for a new command is FIX SOON:
  the author never ran the binary.
- A Testing section that says the e2e suite passed is checked against the CI
  E2E run on the head commit, not re-run here. Never run write e2e tests from
  a developer machine as part of a review.

## Part 6: Report

Print the report to the terminal. Also save it to `$S/report.md` (with
`--no-drive`, part 4 never set `S`: use `mkdir -p /tmp/pr-review-<n>`). With
`--comment`, post the summary as one PR comment and each BLOCKER and FIX SOON
as an inline comment on the first line of the hunk it concerns.

Verdict rule: **BLOCK** if there is at least one BLOCKER. Otherwise **PASS**.
FIX SOON items do not block but are listed first after blockers.

**Human-in-the-loop score.** Decide how much human attention the PR needs
and say why. Add the points below, cap at 10, and print the tier with the
top three reasons as **Highlights**. The score is advisory until the team
sets a merge policy; it never replaces the verdict.

| Signal | Points |
|--------|--------|
| Any BLOCKER | tier **review**, regardless of score |
| `sensitive` row, unexplained / explained | +3 / +1 |
| `judge` row in core or another command, unexplained | +2 |
| New write verb added | +3 |
| New dependency in `Cargo.toml` | +2 |
| Schema removal or rename | +3 |
| Drive step failed, or a Testing-section command failed | +3 |
| E2E check red, or e2e not added/updated for a changed API command | BLOCKER, +3 |
| Missing unit or integration layer / missing deserialization test | +1 / +2 |
| Each FIX SOON (cap 3) | +1 |
| PR template sections missing | +1 |
| Diff over 500 / over 1500 lines | +1 / +2 |
| Author not an org member, or CLA unsigned | +2 / +1 |

A same-repo PR (`headRefName` without a fork prefix in `gh pr view`) comes
from someone with write access: count the author as an org member without
an API lookup. The CLA state is the `license/cla` line of `gh pr checks`.

| Score | Tier | What the human does |
|-------|------|---------------------|
| 0-2 | **auto** | Nothing. Auto-merge candidate once policy allows. |
| 3-5 | **glance** | Read the Highlights and the What-I-ran table. Two minutes. |
| 6-10 | **review** | Domain owner reviews the diff. The bot's findings are the agenda. |

Put the tier and highlights on the line after **Scope** in the report.

Format, exactly:

```markdown
# PR Review: #<n> <title>

**Verdict: 🟢 PASS | 🔴 BLOCK**
**Scope:** This PR is about X.
**Human review: 🟢 auto | 🟡 glance | 🔴 review (score N of 10).** Highlights: ...

## Blockers
- `path/file.rs:123` — <claim>. Failure: <inputs> → <wrong result>. Evidence: <command or quote>.

## Fix soon
- ...

## Cleanup
- ...

## Scope table
| file | domain | footprint | explained | severity |
(only judge / sensitive rows and any row with a note)

## Interface delta (cx schema)
- added: ...
- removed / renamed: ...

## What I ran
**Checks: N passed, M failed.**
| command | expected | result | note |
(expected = what a correct build does, e.g. "exit 0, 3 formats" or
"exit 1, clear 400 error"; result = 🟢 PASS or 🔴 FAIL, never the raw exit
code alone, so a reader sees at a glance whether every check held. Use the
same circles on the Verdict line: 🟢 PASS / 🔴 BLOCK, and on the tier line:
🟢 auto / 🟡 glance / 🔴 review.)

## Test layers
| command | unit | integration | e2e | skill |
(present / missing per changed command. The e2e cell says one of:
`updated: <test name>`, `added: <test name>`, `missing`, or
`exempt: <reason>`, plus the CI E2E check result for the head commit.)

## PR template
Missing or empty sections: <list or none>

## Not verified
- <what, and why>
```

Rules for the text: one claim per bullet, a file and line on every code
finding, no finding without evidence, no praise, no restating the diff.

**Structured result (CI).** When the run asks for a JSON result
(`--json-schema`), return the same decision the report shows:

| Field | Value |
|-------|-------|
| `verdict` | `PASS` or `BLOCK`, as on the Verdict line |
| `blockers` | the number of bullets under **Blockers** |
| `tier` | `auto`, `glance`, or `review`, as on the Human review line |
| `score` | the score N from the Human review line, 0 to 10 |
| `report` | the full summary comment, in the format above |

The CI gate reads only this result, and fails when it disagrees with the
report's Verdict line, with the blocker count, or with the tier table above.
In CI, do not post the summary comment yourself: the workflow posts
`report` and adds the `**Commit:**` line. CI has no git. The workflow writes
the footprint, the diff, the commits, and the PR's files to the paths the
prompt names. Read them with Read, Grep, and Glob instead of the git commands
in this skill. For example, search `tests/e2e` in the PR's files with Grep
instead of `git grep`.

## Cleanup

```bash
git worktree remove --force "$S/head"; git worktree remove --force "$S/base"
git update-ref -d refs/pr-review/<n>
```

Skip this when part 4 was skipped: there are no worktrees, and the CI
runner is thrown away with its refs.

Leave `$S/report.md` and the two binaries in place; they are in the scratchpad
and cost nothing.

## Local tips

- First run on a machine is slow: `cargo build` needs several minutes cold.
  Later runs reuse `target/` and take under a minute per binary.
- To review your own uncommitted work, commit it first; the skill reviews refs.
- `CX_REVIEW_PROFILE=c4c-staging /pr-review 292` sets the profile up front and
  skips the question.
