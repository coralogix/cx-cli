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

Get the full picture of what the PR will contain - look at ALL commits on the branch, not just the latest one:
```bash
git log --oneline <base-branch>..HEAD
git diff <base-branch>...HEAD
```

If the diff is very large, also read the individual commit messages for context since they often explain intent better than raw diffs.

### 5. Create the PR

Generate a concise title (<70 chars) and a short summary body. The title should describe the change at a high level. The body should have a few bullet points explaining what changed and why.

Use this format:
```bash
gh pr create --title "the title" --body "$(cat <<'EOF'
## Summary
- First change
- Second change

EOF
)"
```

If the base branch is not the default, pass `--base <branch>`.

### 6. Report back

Print the PR URL so the user can click through to it.
