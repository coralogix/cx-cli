# cases -- manual items

`cases` has no create/list-your-own-object of its own. Every subcommand in
the original session (`OLD_DIR/results/cases.jsonl`, 42 entries) ran against
3 REAL, pre-existing demo-team cases:

- `8623c2d0-2d1b-5094-a3b6-d454c112e5d9` (became CASE-43)
- `b0ea9128-e734-58a3-a160-cf46e357a9ed` (became CASE-96)
- `ee1dca07-c329-5250-917f-3b0374f86af0` (became CASE-97)

All 3 were resolved and then **closed** during that run ("per explicit
approval to run everything" -- a one-time, human-approved decision for that
session, not a standing license). There is no undo for CLOSED status. Per
the task's classification rules, ANY subcommand that mutates a real case's
state must be MANUAL -- never auto-mutate real production cases on a
schedule with no throwaway target to pick. Only the 4 genuinely read-only
lookups (`get`, `events list`, `events get`, `notifications`) are in
`../automated/cases.py`.

Every item below passed cleanly last time (status PASS, exit 0, empty
`notes` unless stated) -- the issue isn't reliability, it's that mutating a
real case is inherently a judgment call, every single time.

## `cases update <id> --title ...`

```
cx -p kb-demo -o <fmt> --yes cases update <case-id> --title "<new title>"
```

**Why manual:** overwrites the real title of a real case. Baseline: PASS,
e.g. `cases update 8623c2d0-... --title "[otel-demo] Error Log Rate - PR176 smoke test"`.
**Criteria:** only run this against a case you specifically intend to
relabel; never on an automated schedule.

## `cases comment <id> --text ...`

```
cx -p kb-demo -o <fmt> --yes cases comment <case-id> --text "PR176 console-link smoke test comment"
```

**Why manual:** appends a permanent, real comment to a real case's history.
Baseline: PASS. **Criteria:** repeating this on every automated run would
spam the case's audit trail forever with no way to retract; only do it
deliberately, once, when you need to check the comment endpoint's
`consoleUrl`.

## `cases assign <id> --user ...` / `cases unassign <id>`

```
cx -p kb-demo -o <fmt> --yes cases assign <case-id> --user <email>
cx -p kb-demo -o <fmt> --yes cases unassign <case-id>
```

**Why manual:** reassigns a real case's owner, which can affect who's
paged/notified. Baseline: PASS for both. **Criteria:** only run against a
case where you've confirmed reassigning (even temporarily, even back to the
same user) won't confuse a real on-call workflow.

## `cases acknowledge <id>` / `cases unacknowledge <id>`

```
cx -p kb-demo -o <fmt> --yes cases acknowledge <case-id>
cx -p kb-demo -o <fmt> --yes cases unacknowledge <case-id>
```

**Why manual:** flips a real case's acknowledgment state, which is part of
real incident-response signaling. Baseline: PASS for both. **Criteria:**
same as assign/unassign -- fine as a one-off deliberate check, not a
standing automated toggle.

## `cases set-priority <id> --priority P2` / `cases clear-priority <id>`

```
cx -p kb-demo -o <fmt> --yes cases set-priority <case-id> --priority P2
cx -p kb-demo -o <fmt> --yes cases clear-priority <case-id>
```

**Why manual:** changes a real case's priority, which can affect SLA/
escalation behavior downstream. Baseline: PASS for both. **Criteria:** same
reasoning as the other toggles above.

## `cases resolve <id> --reason ...`

```
cx -p kb-demo -o <fmt> --yes cases resolve <case-id> --reason "PR176 console-link smoke test - resolving real demo case"
```

**Why manual:** resolves a real case. Baseline: PASS, with the explicit
note "Resolved a real demo-team case (CASE-43/96/97) as part of
console-link verification, per explicit approval to run everything."
**Criteria:** only run with the same kind of explicit, informed human
approval given last time -- never as an unattended replay.

## `cases close <id>`

```
cx -p kb-demo -o <fmt> --yes cases close <case-id>
```

**Why manual:** closes a real case, and CLOSED has **no undo** on this
backend. Baseline: PASS, with the explicit note "Closed a real demo-team
case (CASE-43/96/97) as part of console-link verification, per explicit
approval to run everything." **Criteria:** this is the single highest-risk
item in the whole `cases` group -- require explicit, case-specific human
sign-off every time, never bundle it into a routine re-run.
