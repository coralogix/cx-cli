# alerts -- manual items

None. Every subcommand exercised in the original session
(`OLD_DIR/results/alerts.jsonl`, 20 entries) passed cleanly on the first try
with a known-working payload and lifecycle: `create` (setup) -> `get` /
`enable` / `disable` / `list` / `events` / `event-stats` across
text/json/agents -> `delete` (cleanup). There were no FAILs, no ambiguous
output, and no irreversible or judgment-laden steps.

All of it is replayed mechanically by `../automated/alerts.py`.
