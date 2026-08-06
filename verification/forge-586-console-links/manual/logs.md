# logs -- manual verification items

## Summary

None. All 5 entries in `OLD_DIR/results/logs.jsonl` are `PASS`, read-only
DataPrime queries with no side effects. `automated/logs.py` replays all of
them exactly (the 3-format matrix, the explicit `--tier frequent` check, and
the explicit default-tier check).
