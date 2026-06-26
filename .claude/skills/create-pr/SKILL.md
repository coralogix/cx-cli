---
name: create-pr
description: Creates GitHub pull requests with auto-generated summaries. Use this skill whenever the user wants to create a PR, open a pull request, submit changes for review, or push their branch for review - even if they don't explicitly say "PR".
metadata:
  internal: true
---

# Create Pull Request

Create a GitHub pull request for the current branch with an auto-generated title and summary.

## Prerequisites

Verify `gh` is installed and authenticated by running `gh auth status`. If it fails, tell the user to run `gh auth login`.

## Workflow

### 1. Understand the current state

Run these in parallel:
- `git status` - check for uncommitted changes
- `git log --oneline -20` - recent commit history
- `git rev-parse --abbrev-ref HEAD` - current branch name
- `git remote show origin | grep 'HEAD branch'` - detect the default base branch (usually `main` or `master`)

### 2. Handle uncommitted changes

If there are staged or unstaged changes, ask the user whether they'd like to commit them before creating the PR. Do not commit or push without explicit approval.

### 3. Handle pushing

Check if the current branch has a remote tracking branch and is up to date:
```bash
git rev-parse --abbrev-ref --symbolic-full-name @{u} 2>/dev/null
```

If the branch isn't pushed or is ahead of the remote, ask the user before pushing. Push with `-u` to set up tracking.

### 4. Analyze changes

Most of the code in this repo is LLM-generated, so reviewers lean on the PR description more than the diff. Your job here is to gather enough material to fill **every** section of the template in step 5 — not just enumerate what changed.

Look at ALL commits on the branch, not just the latest one:
```bash
git log <base-branch>..HEAD                 # full messages, not just --oneline
git diff <base-branch>...HEAD
git rev-parse --abbrev-ref HEAD             # branch name often carries a ticket ID
```

While reading, deliberately collect:
- **Why** the change exists (commit messages, branch name, linked tickets)
- **How** it works at a high level (which modules/layers, data/control flow)
- **Non-obvious decisions** (anything a reviewer might question)
- **Behavioral / interface / schema changes** (what's different about the system after merge)
- **What was actually tested** (look for test files added, but verify with the user / run history)
- **Risks** (breaking changes, migrations, blast radius)

If you don't have material for a section, don't invent it — that section becomes `N/A — <one-line reason>` in step 5.

### 5. Create the PR

Generate a concise title (<70 chars) describing the change at a high level.

The body **must follow the template below exactly**. All 8 section headers must appear in order. If a section truly doesn't apply, write `N/A — <one-line reason>` under that header — never omit a header, never leave a section empty.

Template (fill in under each header):

````
## Context
Why this change exists. What problem it solves or capability it adds.
Reference the trigger (bug, feature request, incident, refactor goal).
One short paragraph — not a changelog.

## Linked Issues
Linear tickets, GitHub issues, incident IDs, or prior PRs this builds on.
`N/A — <reason>` if none.

## Design
The high-level shape of the implementation. How the pieces fit together,
which modules/layers are touched and in what role, and the data/control
flow for the main path. Aim for what a reviewer would draw on a
whiteboard. Avoid restating the diff line-by-line.

## Key Decisions
Non-obvious choices and the trade-offs behind them. Alternatives
considered and why they were rejected. Anything a reviewer might
otherwise flag as "why did you do it this way?" — answer it here
pre-emptively. `N/A — <reason>` only for trivial/mechanical changes.

## Changes
Logical/semantic changes — not a file-by-file inventory. Focus on:
- Interface changes (new/changed/removed commands, flags, public APIs)
- Schema changes (config, data model, API request/response shapes)
- Behavioral changes (what `cx` does differently now)
Skip mechanical detail (which files moved, which libs bumped) unless
it materially affects reviewers. Rollout concerns (migrations,
breaking-change handling) belong in Risks & Rollout, not here.
Bullet list.

## Testing
How the change was verified. List the exact commands run (unit,
integration, e2e, manual `cx` invocations). If a layer was skipped,
say why. Aspirational tests don't count — only what was actually
executed.

## Risks & Rollout
Backwards compatibility, breaking changes, migrations, feature flags,
performance impact, blast radius if this goes wrong. How to roll back.
`N/A — <reason>` for low-risk changes (e.g. pure docs, isolated
refactor).

## Out of Scope / Follow-ups
What this PR deliberately does *not* do, and any follow-up work that
should land separately. `N/A — <reason>` if the PR is fully
self-contained.
````

Invoke with:
```bash
gh pr create --title "the title" --body "$(cat <<'EOF'
## Context
...

## Linked Issues
...

## Design
...

## Key Decisions
...

## Changes
- ...

## Testing
- ...

## Risks & Rollout
...

## Out of Scope / Follow-ups
...
EOF
)"
```

If the base branch is not the default, pass `--base <branch>`.

### 6. Pre-submit checklist

Before running `gh pr create`, verify the body you assembled:

- [ ] All 8 section headers are present, in order: Context, Linked Issues, Design, Key Decisions, Changes, Testing, Risks & Rollout, Out of Scope / Follow-ups
- [ ] No section is empty — each has either real content or `N/A — <reason>`
- [ ] Testing lists commands that were **actually run** for this branch, not generic suggestions
- [ ] Changes describes logical/semantic differences, not a file inventory
- [ ] Design says how the change works at a level above the diff

If anything fails, fix the body before submitting.

### 7. Report back

Print the PR URL so the user can click through to it.
