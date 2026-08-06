# completions - manual verification items

## `completions generate elvish`

**Command:**

```
cx -p kb-demo -o text completions generate elvish
```

**Why it needs judgment:** FAILed last time with `Error: Shell 'elvish' is not supported
by cx completions`. This looks like a deliberate, stable input-validation rejection (not
a flaky network/backend issue - `completions` makes no API calls at all), but per policy
any FAIL still needs a human to confirm "same known limitation" vs. "something changed"
before being treated as an expected, permanent non-issue.

**Decision criteria:**
- Re-run and compare the exact error text. Unchanged -> elvish is still an intentionally
  unsupported shell, no action needed.
- If it now succeeds (prints a completion script) or the error message/wording changes
  -> check whether elvish support was added or the error handling changed, and consider
  promoting `generate elvish` into `automated/completions.py`'s format loop.

**Known baseline** (from `OLD_DIR/results/completions.jsonl`):
- status: `FAIL`
- stderr: `Error: Shell 'elvish' is not supported by cx completions`

## `completions refresh`

**Command:**

```
cx -p kb-demo -o text completions refresh
```

**Why it needs judgment:** `refresh` has no `--path`/scratch override (confirmed via
`--help` in the original run) - it regenerates ALL previously-installed completions
tracked in the real `~/.cx/config.toml` under `managed_completions`. On a real machine
this includes the user's actual registered shell completion file (e.g. `~/.zfunc/_cx`
for zsh). Running it would silently overwrite that real file. The old run explicitly
skipped this subcommand for exactly this reason, and it must stay skipped in any
mechanical replay - there is no throwaway target to redirect it to.

**Decision criteria:**
- Only test this manually, on a machine/environment where you've confirmed
  `~/.cx/config.toml`'s `managed_completions` list contains no real, depended-upon
  completion file paths (e.g. a fresh container/VM), or where you're prepared to
  regenerate/restore whatever it touches afterward.
- Never wire this into an unattended/automated script.

**Known baseline** (from `OLD_DIR/results/completions.jsonl`):
- status: `SKIPPED`
- notes: "refresh has no --path/scratch override (see --help) - it regenerates ALL
  previously-installed completions tracked in ~/.cx/config.toml managed_completions,
  which includes the user's real zsh entry at ~/.zfunc/_cx. Would overwrite the user's
  real completion file, so skipped per task instructions."
