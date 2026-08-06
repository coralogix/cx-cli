# cleanup -- manual verification items

## Summary

None. `cleanup` is a single, no-argument, idempotent local housekeeping
command with no console link and no side effects beyond deleting its own
stale spill files. All 3 entries in `OLD_DIR/results/cleanup.jsonl` (text,
json, agents) are `PASS`, and `automated/cleanup.py` replays all three
exactly.
