# retentions - manual verification items

## `retentions activate` (IRREVERSIBLE - already flipped on kb-demo)

**Command:**

```
cx -p kb-demo -o json --yes retentions activate
```

**Why it needs judgment:** this call flips `enableTags` from `false` to `true` for the
whole team. The old run already executed it once (deliberately, "run once per
instructions") and then tried to revert it via `retentions update` - which 501s on this
team/plan, and there is no `deactivate`/opposite subcommand exposed by the CLI. So
`enableTags` is now permanently `true` on the "kb-demo" team with no way back through the
CLI. **This must never be auto-replayed** - on kb-demo it would be a harmless re-flip of
an already-true flag, but the whole point of keeping this manual is to stop the pattern
of running it unattended: on any other team (or if kb-demo's config is ever fixed
upstream) it would flip another flag with no revert path.

**Decision criteria:**
- Do not run `retentions activate` against any team unless a human has explicitly
  decided the resulting irreversible state change is acceptable for that team.
- If investigating this area, use `retentions status` (read-only, safe, in
  `automated/retentions.py`) to check current state first.

**Known baseline** (from `OLD_DIR/results/retentions.jsonl`):
- `status (baseline, before activate)`: `PASS` - notes: "BASELINE ORIGINAL VALUE:
  enableTags=false. Recorded for manual revert reference."
- `activate`: `PASS` (exit 0) - notes: "LIVE MUTATION, run once per instructions. ...
  Changed enableTags false -> true. Attempted to revert via `retentions update` (see next
  entry) but that endpoint returns 501 Unimplemented on this team, and there is no
  `deactivate`/opposite subcommand exposed by the CLI, so enableTags COULD NOT BE
  RESTORED to its original false value. This is a real residual side effect on the live
  demo team: enableTags is now true. Flagging for the user."
- Current state on kb-demo going forward: `enableTags=true` (confirmed by the `status`
  calls immediately after `activate`, and expected to still read `true` on any fresh
  `automated/retentions.py` run).

## `retentions update` (known 501 baseline)

**Command:**

```
echo '{"retentions":[{"id":"5ed15614-4f2a-47f6-b8e2-5e6e29a3410c","order":2,"name":"Short","editable":true}]}' \
  | cx -p kb-demo -o json --yes retentions update
```

**Why it needs judgment:** stable, known 501 Unimplemented for this team/plan, confirmed
with both a real-looking body and an empty `{}` body. This is also the same endpoint that
would have been the only way to revert `activate` above, so it's doubly relevant to
re-check if this team's plan is ever upgraded.

**Decision criteria:** re-test manually if you need to confirm whether this team's plan
now supports the endpoint (e.g. before attempting to use it to finally revert the
`enableTags` flip); otherwise leave as-is, it's a stable known limitation.

**Known baseline** (from `OLD_DIR/results/retentions.jsonl`):
- status: `FAIL`
- notes: "Backend returns 501 Unimplemented for this team/plan regardless of payload
  shape (also tried with an empty {} body - same 501). This looks like a genuine backend
  limitation (feature not enabled for this demo team), not a bug introduced by PR176's
  console-link changes. Could not use this to revert the `activate` mutation above."
