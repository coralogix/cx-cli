# olly - manual verification items

## `olly ask`

**Command:**

```
cx -p kb-demo -o json olly ask "What services have I got?"
```

**Why it needs judgment:** this is a real call to the Coralogix AI assistant - it costs
real tokens every time it runs, and the response is inherently non-deterministic (it's a
live LLM answer over this team's real telemetry). The old run intentionally limited
itself to a single exchange in `json` format only, and explicitly skipped repeating the
call in `text`/`agents` format "to avoid repeated real AI cost" - the same reasoning
means it should not be re-run automatically/repeatedly just to regenerate console-link
verification data.

There's an added wrinkle: the artifact-looking id embedded in the `ask` response's
markdown link (`8ae1d5fb-e388-4a5c-a4e3-9836f8a0c8f9`) 404s against `olly artifacts get`
- it appears to reference a different resource type (a UI-side insight/service-map view),
not a stored artifact. A human needs to look at whatever a fresh `ask` response contains
and judge whether its embedded links/ids make sense, rather than mechanically chasing
whatever id shows up.

**Decision criteria:**
- If re-verifying this command, do it as a single deliberate call (not looped/repeated),
  and check: does the response include a `chat_id`? Does it include a `consoleUrl`? Does
  any artifact-style link in the response actually resolve via `olly artifacts get`?
- If a linked id now resolves via `artifacts get` (i.e. the mismatch noted above is
  fixed), that's worth noting as a behavior change but doesn't need urgent action.
- Do not build a loop or scheduled job that calls `olly ask` repeatedly for verification
  purposes - the cost and non-determinism make it unsuitable for that.

**Known baseline** (from `OLD_DIR/results/olly.jsonl`):
- `ask` (json): `PASS` - notes: "Real AI assistant call (costs tokens) -- limited to one
  exchange in json format only per instructions to avoid repeated real AI cost."
- `ask (text format)` / `ask (agents format)`: `SKIPPED` - "Skipped repeat real-AI-cost
  call; already verified consoleUrl + response shape via the json-format run above."
- `artifacts get` (using the id from the `ask` response, `8ae1d5fb-...`): `FAIL` (all
  formats) - see `artifacts get (real pre-existing artifact id)` entry which succeeded
  instead using a real id (`30fbcbe6-...`) taken from `artifacts list` - notes: "The
  artifact id linked from the `olly ask` response 404'd on this endpoint (likely a
  different resource type, e.g. UI insight link, not a stored artifact); used a real id
  from `artifacts list` instead."
