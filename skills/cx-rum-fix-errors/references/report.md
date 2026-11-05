# Report and PR Templates

Never include user email, name, ID, IP, session ID, tokens or raw log lines. Quote the error message only after removing any user values in it.

## Final report

Use this structure at the end of every run (`investigate` and `fix`):

```markdown
## RUM error: <error message, user values removed>

- **Template ID:** <template-id>
- **Application:** <app> · <platform> · versions <first-seen> → <last-seen>
- **Impact:** <errors> errors, <users> users, <sessions> sessions, <time range>
- **Status:** fixed | investigated (no change) | not fixable here: <class and reason>

### Root cause
<one-line summary>

- **Immediate cause:** <expression and the value it had, file:line>
- **Root cause:** <why the value is wrong at that moment>

### Evidence
- **Mapped frames:** <top in-app frames, file:line function>
- **Release:** commit <sha> in <repo>; local repo matches: yes | no (<detail>)
- **Breadcrumbs:** <the user actions and requests that led to the error>
- **Network:** <failed requests, status codes> | none
- **Backend trace:** <service, operation, error> | none
- **Spread:** <browsers, pages, versions>

### Alternatives ruled out
- <alternative> - <why>

### Changes
- `<file>` - <what changed>
- `<test file>` - regression test: <the failing condition it reproduces>

### Verification
- [ ] Test reproduces the exact failing condition and fails without the fix
- [ ] Edge cases covered: <list>
- [ ] Lint and tests pass: <commands run>
- [ ] No regressions in other callers: <what you checked>
- [ ] This fixes the root cause, not a symptom (or: symptom only, because <reason>)

### Follow-ups
- <same pattern elsewhere, backend issue to file, config change, missing source maps>

[View in Coralogix](<Coralogix link from the prompt, if any>)
```

For several errors, write one section per error, then a summary table: template ID, status, reason.

## PR body template

Only when the user asks for a PR. Open it as a **draft**. Title: `fix(<area>): <what was wrong>`.

```markdown
## Summary
Fixes a production error reported by Coralogix RUM: <error message, user values removed>.

<one paragraph: root cause and the fix>

## Impact
<errors> errors, <users> users, <sessions> sessions between <start> and <end>. First seen in <version>.

## Root cause
- **Immediate cause:** <expression, file:line>
- **Root cause:** <why>

## Evidence
- Mapped stack (release <sha>): <top in-app frames>
- <breadcrumbs / network / backend trace, short>

## Changes
- `<file>` - <what changed>

## Testing
- Regression test `<test name>` reproduces <the failing condition> with synthetic data
- <lint and test commands run, and results>

## Follow-ups
- <optional>

cx-rum-error: <template-id>
```

Keep the `cx-rum-error: <template-id>` line exactly as shown, on its own line. Coralogix uses it to link the PR to the error and show the release that fixed it.
