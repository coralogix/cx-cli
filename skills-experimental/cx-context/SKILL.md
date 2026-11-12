---
name: cx-context
description: >
  Use this skill to read and maintain shared Coralogix team knowledge with
  cx context: factual memories and team instructions. Use it alongside
  other cx skills when answering observability questions, investigating alerts,
  cases or incidents, or changing Coralogix resources, even when the user does
  not mention memories. Triggers include "what do we know about this service",
  "check team memories", "read team instructions", "save what we learned",
  "remember this finding", "correct a memory", "add a team instruction",
  "undo a context change". Not for unrelated code or local-machine
  troubleshooting, or for the agent's personal memory.
metadata:
  version: "0.2.0"
  internal: "true"
---

# Team context: memories and instructions

## Notice

Unreleased experiment with no stable contract. `cx context` is not listed in `cx --help` or `cx schema` yet, but it runs as shown here; nothing has to be set. If `cx context get` fails as an unknown command, or answers that agent context is not available in this environment, carry on with the task without it and say so.

## What team context is

A Coralogix team shares one pool of entries, written by teammates and their agents and read by all of them:

- A **memory** is factual: something observed or learned earlier, true when it was written. Use it to decide what to check; verify it against current data before relying on it.
- An **instruction** is guidance from the team. It may state facts, and it may also say what to do or avoid. Follow the instructions that apply to the task.

Each entry is attached to **subjects**, or is **team-wide**. A subject is `type:value` (`service:checkout`, `alert:<definition ID>`, `host:ip-10-0-1-17`), or a type alone (`service`) for knowledge about all of that type.

## Reading

```bash
cx context get service:checkout alert:8f3a2c1e-5b7d-4e2a-9c1f-0d6b3a4e5f60
cx context get                       # team-wide instructions and recent memories
```

`get service:checkout` returns the entries attached to `service:checkout` or to the type alone (`service`), plus team-wide instructions. It prints instructions in full, and one index entry per memory: ID, date, subjects, title and summary. Open the memories that look relevant:

```bash
cx context memories get <ID>
```

One read rarely finds everything. Read again with subjects that turn up during the work, and search by symptom, error text or component name:

```bash
cx context get host:ip-10-0-1-17 ip:10.0.1.17
cx context memories search timeout -e 'connection pool' -e retry
cx context instructions search -F 'checkout-db'
cx context memories list --subject service:checkout
```

Search covers title, summary and body, line by line, as a case-insensitive regular expression (`-F` for a fixed string). In `list` and `search`, `--subject` matches only that exact subject. Both return index entries, `search` with the matching lines cut short: read an entry in full with `cx context memories get <ID>` or `cx context instructions get <ID>` before relying on it. A search hit on an instruction for another subject is not automatically meant for this task: name the subject it belongs to and judge whether it applies.

Add `-o json` or `-o toon` for machine-readable output. `cx context` works with one profile per call.

## Common uses

- **Answering a question about the team's data.** Saved context may explain what to count or where to look, for example that some failures are returned with status 200 and an error body. Looking it up first can save wrong queries. The memory tells you how to measure; the current numbers come from the data.
- **Investigating an alert, case or incident.** Saved context for the subjects at hand, and for subjects found along the way, can point to where to look and what was already ruled out. In the result, name the memories you relied on by title and say whether today's evidence supports or contradicts them. A past finding is not proof of today's cause.
- **Saving what was learned.** When the work turned up something useful that the next person or agent would not find as easily, offer to save it, also when the work did not succeed or finish. Skip what is just as easy to find again.
- **Correcting.** When a memory is wrong or out of date, update it instead of adding a second one.
- **Team instructions.** Create or change them only when the user asks. You may suggest one while working with a user.

## Saving a memory

Show the user the exact title, summary, subjects and body, and save only after they approve; pass `--yes` only then. Approval to investigate is not approval to save, and changed text needs approval again. If the user approved saving in advance, that counts. If nobody can answer, do not save: put the proposed memories in the final result, unsaved.

```bash
cx context memories create --yes \
  --title 'Checkout pool waits during the November 8 deployment' \
  --summary 'Production checkout timeouts coincided with pool saturation; trigger unknown.' \
  --subject service:checkout \
  --source 'case 6d1f0c2a-3b4e-4f5a-8b9c-7e6d5c4b3a21' \
  --body-file note.md                # or --body-file - to read stdin
```

If a create fails without a clear answer, retry it with the ID the error names (`--id <UUID>`) and exactly the same title, summary, body, source and subjects: that does not save twice. Changed content with the same ID is refused; do not drop `--id` unless you are sure the first attempt saved nothing. Say a memory was saved only after `cx` confirms it.

What to write:

- Compact facts or short bullets; one sentence is enough for a small fact.
- A title and summary that let a reader decide whether to open it, with the distinguishing symptom, component and scope.
- When it was observed and where it comes from. The saved date is not the observation date, and `--source` is not searched, so put the words someone would search for in the title, summary or body.
- When a memory holds both observations and conclusions, mark the lines `Observed:` and `Inferred:`, and state what is still uncertain. Do not present a hypothesis as a cause. For unfinished work, record what was checked and what was ruled out.
- Exact names, error text, and the queries or `cx` commands that reproduce the finding.
- Only the structure the content needs. The body must make sense without this conversation.

Two bodies of different shape:

```text
Observed: 2026-11-08 14:03-14:11 UTC, production checkout: all 40 DB connections per pod in use; pool-wait p95 2.4 s, request timeouts 8%.
Observed: after rollback at 14:11 UTC, pool-wait p95 fell below 50 ms by 14:14.
Inferred: pool contention likely contributed; the data does not separate higher demand from slower DB operations.
Scope: one deployment; not verified for others.
```

```text
As of 2026-11-08, production checkout sets db.pool.max=40 connections per pod (deployment config revision cfg-184).
```

Search before saving. A new observation is a new memory; never merge conflicting observations silently.

## Subjects

- Type names and IDs exactly as `cx` shows them: `service:checkout`, `dataset:logs`.
- An alert by its definition ID; a case by its UUID (`cx cases get CASE-123` resolves the short form).
- Things without a Coralogix ID as open types: `ip:10.0.1.17`, `host:ip-10-0-1-17`.
- A type alone (`--subject service`) only for knowledge about all of them.
- `--team-wide` only on purpose.

## Instructions

```bash
cx context instructions create --yes --title 'Page the payments on-call' \
  --summary 'Read before escalating a checkout incident.' \
  --subject service:checkout --body-file instruction.md
```

If two instructions contradict each other, show both to the user; do not pick one.

## Privacy

Everything saved is shared with the whole team. Leave out usernames, local paths, secrets and personal data.

## Changing and undoing

`update`, `delete` and `revert` name the revision they are based on; take it from `get` and pass it as `--expected-revision`. A conflict means someone else changed the entry: read it again and reconsider the change.

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

`update` keeps the content fields not given; `--change-note` applies to that revision only. `--subject` replaces all subjects, `--team-wide` removes them, `--no-source` clears the source. The same commands exist under `cx context instructions`.
