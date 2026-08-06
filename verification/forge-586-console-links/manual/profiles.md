# profiles -- manual verification items

## 1. `profiles add` (and the `set-default`/`delete` steps that depend on it)

**Command:**
```
cx profiles add cx-harness-test
```
(no `-p kb-demo` -- `profiles add` is a local, profile-agnostic command)

**Why it needs judgment:** `profiles add` is fully interactive -- it drives
an `inquire`-based TUI wizard (`Select`/`Password`/`Text`/`Confirm` prompts)
with **no non-interactive flags** (confirmed via `--help`: only `[NAME]` and
`--set-default` exist). Piped/non-TTY stdin fails immediately with `input
device is not a TTY`. The original run drove it with `expect` sending raw
keystrokes (Down+Enter, typed text, more Down+Enter, plain Enter to accept
defaults, `n`+Enter) -- but the JSONL's `command` field only records a prose
description of that interaction (`"expect-driven: ... (Authentication
method=API key, key=dummy-test-key-12345, Region=eu2, Label=<empty>,
storage=file, output format=text, set-as-default=No)"`), not the literal
expect script or keystroke sequence. There is nothing mechanical to copy —
reconstructing and trusting a TUI-driving script (`expect`, `pty`, or
`pexpect`) without ever being able to run it first is exactly the kind of
judgment call this file exists for.

**Reconstructed prompt flow** (read directly from
`src/commands/profiles/mod.rs::run_add` / `configure_api_key`, current as of
this session -- use this as the starting point, not as a pre-verified
script):

1. `Authentication method:` -- `Select`, starts on `OAuth (browser login)`
   (cursor 0). Send **Down, Enter** to land on `API key (paste manually)`.
2. `Coralogix API key (Team Key or Personal Key):` -- `Password` (masked,
   no confirmation). Type a dummy value (e.g. `dummy-test-key-<suffix>`),
   **Enter**.
3. `Region:` -- `Select` over `["us1","us2","us3","eu1","eu2","ap1","ap2","ap3"]`,
   `with_starting_cursor(2)` (index 2 = `us3`; the inline `// eu1` comment
   in the source is stale/wrong for the current array order -- don't trust
   it, trust the actual index). To land on `eu2` (index 4): **Down, Down,
   Enter**.
4. `Label (e.g. 'prod'):` -- `Text::prompt_skippable`. Just **Enter** with
   no text -- submits `""`, which the code filters to `None`.
5. `Where should API keys be stored?` -- `Select`, starts on `file` (cursor
   0, the desired choice). **Enter**.
6. `Default output format:` -- `Select` over `["text","json","agents"]`,
   starting cursor = index of the *global* `default_output_format`
   (`~/.cx/config.toml`, currently `"text"` on this machine -> index 0, the
   desired choice). **Enter**.
7. `Set 'cx-harness-test' as the default profile?` -- `Confirm`, default
   `false`. Send **`n`, Enter** (or just Enter, since default is already
   No) to decline.

Expect exit 0 and stdout ending `Profile 'cx-harness-test' saved to
~/.cx\nCredentials stored in profile file`.

**Decision criteria:** before trusting any reconstructed automation of this
flow unattended, a human/LLM needs to actually run it once, watch the TUI,
and confirm each prompt lands where expected (cursor positions and prompt
text can silently drift as `inquire`/the option lists change -- e.g. the
stale `// eu1` comment above is proof this has already happened once).
Once a keystroke sequence is confirmed working end-to-end (add -> profile
file written correctly -> `set-default` -> `delete -f`), it can be promoted
into `automated/profiles.py` using `expect` (available at `/usr/bin/expect`
on this machine) or Python's `pty` module.

`set-default cx-harness-test` / `set-default c4c` (to restore) / `delete
cx-harness-test -f` are each trivial, non-interactive, one-liner commands
on their own (`profiles set-default <name>`, `profiles delete <name> -f`) --
they only stay manual because they have nothing to target without a
successful `add` first. `list` (the read-only, argument-free part of this
group) is already covered in `automated/profiles.py`.

**Baseline (2026-08-03):**
- `add` -- `PASS`, exit 0. Notes: "profiles add is fully interactive
  (dialoguer prompts), requires a real TTY ... Drove it with `expect`
  sending: Down+Enter (select API key auth), type
  dummy-test-key-12345+Enter, type eu2+Enter (region), Enter (skip label),
  Enter (keep file storage), Enter (keep text output format), 'n'+Enter (do
  not set as default). Verified profile written correctly to
  ~/.cx/profiles/cx-harness-test.toml."
- `set-default (to throwaway)` -- `PASS`, exit 0.
- `set-default (restore c4c)` -- `PASS`, exit 0. Notes: "restoring real
  default profile back to c4c after throwaway set-default test".
- `list (verify c4c restored as default before cleanup)` -- `PASS`, exit 0.
- `delete (cleanup)` -- `PASS`, exit 0 (`profiles delete cx-harness-test -f`).

This machine's `~/.cx/config.toml` currently shows `default_profile = "c4c"`,
confirming the original run's restore step left things clean.
