# metrics -- manual verification items

## Summary

None. All 12 entries in `OLD_DIR/results/metrics.jsonl` are `PASS`,
read-only PromQL queries with no side effects. `automated/metrics.py`
replays all of them exactly (`query`, `query-range`, `search --name`,
`get-labels`, each across all 3 output formats).

Note: `container_memory_usage_By` is assumed to remain a valid metric name
on the `kb-demo` team. If it's ever removed/renamed, `search --name
container_memory*` (also replayed here) is the fastest way to find its
replacement -- that substitution is mechanical, not a judgment call, so it
doesn't promote this group to manual.
