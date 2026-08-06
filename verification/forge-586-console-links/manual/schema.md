# schema -- manual verification items

## Summary

None. `schema` is a single, no-argument, read-only local command (dumps the
CLI's command tree as JSON for agent discovery) with no console link and no
side effects. All 3 entries in `OLD_DIR/results/schema.jsonl` (text, json,
agents) are `PASS`, and `automated/schema.py` replays all three exactly.
