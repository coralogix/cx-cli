# usage - manual verification items

None. Every `usage` subcommand exercised in the original run (`summary`, `daily`,
`logs-count`, `spans-count`, `capabilities`, `query`, `export-status`) is read-only and
PASSed in all three output formats (see `OLD_DIR/results/usage.jsonl`). All of it is
replayed mechanically in `automated/usage.py`.
