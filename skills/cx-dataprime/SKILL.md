---
name: cx-dataprime
description: |
  DataPrime query language reference for Coralogix. This is a companion skill for detailed syntax
  help. It triggers when the user needs help with DataPrime syntax specifically - how to write
  filters, groupby, aggregations, extract fields with regex, use type conversions, time bucketing
  with roundTime, arrayContains, or asks "how do I write a DataPrime query", "what operators does
  DataPrime support", "how does extract work in DataPrime". This skill is the language reference,
  not the execution guide - if the user wants to actually run a query against a specific data
  source, use the appropriate source-specific skill instead.
version: 0.1.0
---

# DataPrime Query Language

Reference for the DataPrime query language used across Coralogix to search and analyze logs, spans, and other observability data. Covers syntax, commands, operators, and functions.

This skill is the language reference. To actually run queries against a specific data source, use the appropriate source-specific skill instead.

## Quick Reference

A DataPrime query is a pipeline of commands separated by `|`:

```dataprime
filter $m.severity == ERROR | groupby $l.subsystemname aggregate count() as errors | orderby errors desc
```

## Full Reference

See **[DataPrime Reference](references/dataprime-reference.md)** for the complete language documentation:

- Query structure and pipeline syntax
- Data prefixes (`$m`, `$l`, `$d`) and field access
- All commands: `filter`, `groupby`, `choose`, `create`, `extract`, `orderby`, `dedupeby`, `wildfind`, `lucene`, and more
- Operators: comparison, logical, contains (`~`), null checks
- Aggregation functions: `count`, `sum`, `avg`, `min`, `max`, `percentile`, `distinct_count`, etc.
- Type conversions, time bucketing (`roundTime`), multi-value matching (`arrayContains`)
- Text extraction with regex and JSON parsing
- Built-in documentation commands (`cx dataprime list`, `cx dataprime show`)
