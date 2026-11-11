---
name: cx-context
description: This skill should be used when the user starts or finishes an observability investigation, asks to "check team memories", "read team instructions", "what do we know about this service", "save what we learned", "remember this for next time", "add a team instruction", "undo a memory change", or wants to read or write Coralogix team memories and instructions using the cx CLI.
metadata:
  version: "0.1.0"
  internal: "true"
---

# Team context: memories and instructions

## Notice

Unreleased experiment with no stable contract. It needs a `cx` build that has the `context` command, with `CX_FEATURE_FLAG_AGENT_CONTEXT=1` set. Check that `cx schema` lists `context`; if it does not, or the service answers that it is not available, carry on with the investigation without it.

## Before investigating

Read the context for the subjects at hand:

```bash
cx context get service:checkout alert:8f3a2c1e-5b7d-4e2a-9c1f-0d6b3a4e5f60
cx context get                       # team-wide instructions and recent memories
```

Instructions are prescriptive: follow them. Memories are hints from earlier investigations, true when written; check them against current data.

## Widening

The lookup matches subjects exactly. Read again with subjects discovered on the way (service, host, dataset, IP), and search by symptom, error and component words:

```bash
cx context get host:ip-10-0-1-17 ip:10.0.1.17
cx context memories search timeout -e 'connection pool' -e retry
cx context instructions search -F 'checkout-db'
cx context memories list --subject service:checkout
```

A search hit on another subject's instruction is still an instruction: say which subject it belongs to. A missing match is not evidence of absence.

## Offering to save

When the investigation discovered something useful that the next agent would not find as easily, offer it to the user, even if the investigation did not succeed or finish. Do not offer what the next agent would find just as easily. Show the exact title, summary, subjects and body; save only after the user approves, and pass `--yes` only then:

```bash
cx context memories create --yes \
  --title 'Checkout timeouts after deploys' \
  --summary 'Read when checkout times out within an hour of a deploy.' \
  --subject service:checkout --subject alert:8f3a2c1e-5b7d-4e2a-9c1f-0d6b3a4e5f60 \
  --source 'case 6d1f0c2a-3b4e-4f5a-8b9c-7e6d5c4b3a21' \
  --body-file note.md                # or --body-file - to read stdin
```

If a create fails, retry with the ID the error names (`--id <UUID>`); it does not duplicate the entry.

## What a memory holds

- What was observed, kept apart from what was concluded.
- When, and from what source (a case, an alert event, a query).
- Uncertainty, stated.

Search before saving. Update an existing memory when correcting it; a new observation is a new memory. Never merge conflicting observations silently.

## Subjects

- Type names and IDs exactly as `cx` shows them: `service:checkout`, `dataset:logs`.
- An alert by its definition ID; a case by its UUID (`cx cases get CASE-123` resolves the short form).
- Things without a Coralogix ID as open types: `ip:10.0.1.17`, `host:ip-10-0-1-17`.
- A type alone (`--subject service`) only for knowledge about all of them.
- `--team-wide` only on purpose.

## Instructions

Write or change instructions only when the user asks. They may be suggested while working with a user, never as the end-of-investigation step:

```bash
cx context instructions create --yes --title 'Page the payments on-call' \
  --summary 'Read before escalating a checkout incident.' \
  --subject service:checkout --body-file instruction.md
```

Show contradictory instructions to the user; do not resolve them.

## Privacy

Everything saved is shared with the whole team. Leave out usernames, local paths, commands and personal data.

## Changing and undoing

Every write names the revision it is based on; take it from `get` and pass it as `--expected-revision`. A conflict means someone else changed the entry: read it again.

```bash
cx context memories get <ID>
cx context memories update <ID> --expected-revision 1 --yes \
  --summary 'Read when checkout times out after a deploy or a config push.' \
  --change-note 'Also seen after config pushes'
cx context memories delete <ID> --expected-revision 2 --yes
cx context memories history <ID>
cx context memories get <ID> --revision 2
cx context memories revert <ID> --to-revision 2 --expected-revision 3 --yes
cx context history --since now-7d --kind memory
```

`update` keeps the fields not given; `--subject` replaces all subjects, `--team-wide` removes them, `--no-source` clears the source. `delete` prints the `revert` command that restores the entry.
