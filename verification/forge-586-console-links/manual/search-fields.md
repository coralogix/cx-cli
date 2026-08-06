# search-fields -- manual verification items

## Summary

None. All 6 entries in `OLD_DIR/results/search-fields.jsonl` are `PASS`,
read-only Olly Knowledge Base semantic/value field searches with no side
effects. `automated/search-fields.py` replays both queries (`semantic` and
`value` search types) across all 3 output formats exactly.

Note: the original `command` field in the JSONL joins argv with spaces for
display, which makes `search-fields http response status code --dataset
logs --limit 5` look like 4 separate positional words. It is actually a
single quoted `<TEXT>` argument (`"http response status code"`) --
confirmed against `src/main.rs`'s own `--help` examples (`cx search-fields
"http response status code"`). `automated/search-fields.py` passes it as one
argv element accordingly.
