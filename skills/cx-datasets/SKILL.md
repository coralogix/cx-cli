---
name: cx-datasets
description: Dataset discovery and source-selection guidance for DataPrime system and user-defined datasets. Load when the user asks what datasets exist, how to use `cx datasets list`, how to query `system/...` or `default/...` sources, or how to inspect a specific system dataset after discovery. The `cx-telemetry-querying` skill should be loaded alongside this skill.
metadata:
  version: "0.1.0"
---

# Datasets Skill - Discovering system and user-defined datasets

## Dataset categories

Use `cx datasets list` to discover available datasets:

- **System datasets**: `source system/<dataset>`
- **User-defined datasets**: `source default/<dataset>`

## Core workflow

1. Call `cx datasets list` first.
2. Use the exact `source ...` path returned by `cx datasets list`. Never guess a dataset name.
3. Load or use `cx-telemetry-querying` (DataPrime reference) to build the actual query correctly.
4. If the user's question should be answered from a specific dataset, call `cx dataprime query` with that dataset in the `source ...` clause.

## Important notes

- `cx search-fields` is only for logs and spans, not arbitrary datasets.
- For a known system dataset with dedicated business logic or schema guidance, load the relevant domain skill as well.
- For fleet case analytics on `system/labs.cases.state_updates`, also load `cx-cases`.
