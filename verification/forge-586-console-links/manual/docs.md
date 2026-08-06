# docs -- manual verification items

Every subcommand in this group failed in the original run, all with the
same root cause, and none has ever produced a working invocation to
mechanically replay -- `automated/docs.py` is a no-op stub.

## 1. `docs search`

**Exact command:**
```
cx -p kb-demo -o text docs search explore spans --limit 5
cx -p kb-demo -o json docs search explore spans --limit 5
cx -p kb-demo -o agents docs search explore spans --limit 5
```
(the `<QUERY>` argument is really the single string `"explore spans"`; the
JSONL's space-joined `command` field just renders it unquoted.)

**Why it needs judgment:** all 3 formats fail identically with `Error: HTTP
403 Forbidden for https://coralogix.com/docs/llms.txt`. That's the docs
website itself (not the Coralogix product API) rejecting the CLI's outbound
HTTP request -- most likely a bot-protection/User-Agent/rate-limit block on
`coralogix.com`, not a cx-cli or backend API bug. Whether this is still
happening, has gotten worse (e.g. a different error), or has cleared up
(a real docs site policy is outside this repo's control and can change
without any code change here) can only be determined by looking at a fresh
run's actual error and comparing it to the baseline below -- that
side-by-side judgment is exactly what keeps this in `manual/`.

**Decision criteria:**
- Same `HTTP 403 Forbidden for https://coralogix.com/docs/llms.txt` on
  rerun -> still the same known, external, pre-existing issue. Not a
  regression, nothing to act on.
- A *different* error (different status code, timeout, DNS failure, or an
  actual 200 with real search results) -> investigate; either the docs site
  changed its policy (good news, promote `docs search`/`docs fetch` into
  `automated/docs.py` once confirmed stable) or something in the CLI's
  request (User-Agent, headers, retry logic) changed and needs a look.

**Baseline (2026-08-03):** `FAIL` for all 3 formats, exit 1: `Error: HTTP
403 Forbidden for https://coralogix.com/docs/llms.txt`.

## 2. `docs fetch`

**Exact command:**
```
cx -p kb-demo -o text docs fetch user-guides/data_exploration/spans/
cx -p kb-demo -o json docs fetch user-guides/data_exploration/spans/
cx -p kb-demo -o agents docs fetch user-guides/data_exploration/spans/
```

**Why it needs judgment:** same external-403 story as `docs search` above,
against a different URL (`https://coralogix.com/docs/user-guides/data_exploration/spans/index.md`).
The path itself (`user-guides/data_exploration/spans/`) was never confirmed
real either -- it was taken from the command's own `--help` example text
because `docs search` (which is supposed to supply real suffixes to feed
into `fetch`) never returned anything to copy a suffix from. So even if the
403 clears up, a fresh run needs a human/LLM to sanity-check that the
suffix path is still valid documentation, not just that the HTTP call
stopped erroring.

**Decision criteria:** same as `docs search` above -- same 403 = same known
issue; anything else = investigate. Additionally, if it starts returning
200 with real markdown content, confirm the returned page is topically
sensible for `user-guides/data_exploration/spans/` before treating this as
"fixed" -- if `docs search` also starts working, prefer feeding a
freshly-searched suffix into `fetch` rather than trusting the hardcoded
`--help`-example path indefinitely.

**Baseline (2026-08-03):** `FAIL` for all 3 formats, exit 1: `Error: HTTP
403 Forbidden for https://coralogix.com/docs/user-guides/data_exploration/spans/index.md`.
Notes: "path taken from --help example since `docs search` failed with
HTTP 403 (see search entries) so no real path was obtainable from search
output".
