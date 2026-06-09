---
name: single-case
description: Deep single-case investigation. Load when the user asks about one specific case by ID (UUID or readable id like "CASE-123") — e.g. summarize comments, explain who was notified where, judge whether the trigger reflects a real incident, walk the lifecycle. The `alerts` skill should be loaded alongside this skill.
---

# Case Investigation Skill — Drill into a single Case

This skill describes how to investigate **one specific case** end-to-end.

## Mission

- Build a complete picture of a single case: lifecycle, indicators, comments, KPI breaches, notification outcomes.
- Distinguish a **real incident** from noise / misconfiguration / suppression by inspecting the alert(s) that opened the case.

## Scope

**Use this skill ONLY for questions about one specific case.** Focus on a single case ID (UUID or readable id such as `CASE-123`).
The case ID must come from user input or from tools outputs (`get_alerts_object` or `search_alert_definitions`).

## Tools

Reuse the tools already exposed by the `alerts` skill.

---

## Response to user

When summarizing for a user, lead with **status + priority + title + the open alert indicators**, then drill into details as needed. Don't dump the whole record.

`kpi_breaches` and `impacted_entities` are only returned when `get_alerts_object` is called with `is_verbose=True`. Re-fetch the case with verbose mode when the user asks about KPI SLA breaches or impacted APM services/databases.

---

## Judging "is this a real incident?"

This is the core judgment when a user asks "is this a real incident", "is this just noise", or "is this a false positive". You answer it by examining the alert(s) that opened the case, not the case itself.

> The alert-side analysis lives in the `alerts` skill. The steps here are the case→alert handoff, not a replacement.

For each `AlertIndicator`:

1. **Fetch the alert definition.** Always use `latest_alert_version_id` with `get_alerts_object` — this provides the exact alert definition that created the case. `alert_id` will return the current definition which may have changed since the case was created.
2. **Inspect the query and threshold.** Read the type-specific definition. The query + threshold + window tell you what "fires" actually means here. A "high error rate" alert whose query matches `level:DEBUG` or whose threshold is `>= 1` over 5m is almost certainly noise.
3. **Pull the actual triggering evidence.** For logs use `get_event_watch_data` to get the logs that triggered the case. In other cases or if its not enough, use the alert query to fetch the data from the correct time frame. This is the most direct evidence — quote real numbers, not just thresholds.
4. **Permutations:** the list of label combinations that fired. Many permutations on a low-cardinality grouping → broad impact; one permutation on a single label combination → localized issue. Cross-reference with `impacted_entities`.
5. **State and resolve_time.** `state == INDICATOR_STATE_RESOLVED` with a short `resolve_time - trigger_time` delta typically indicates a transient spike. `state == INDICATOR_STATE_NO_DATA` means the underlying signal stopped reporting (often infra-side, not a product incident). `state == INDICATOR_STATE_TRIGGERED` with no resolve → still firing.
6. **Suppression / muted.** If the `AlertIndicator` output includes a `suppression` key, the trigger was suppressed but the case still opened. Check `suppression.type`: `active_suppression_rules` means a suppression rule was active; `alert_definition_muted` means the alert itself was muted. `suppression.suppressed_time` tells you when suppression took effect. Users often miss this — surface it explicitly.
7. **Grouping type.** `grouping.type == GROUPING_TYPE_COMPOSITE_ALERT` and `grouping.type == GROUPING_TYPE_COMBINATION_ALERT` mean correlations — the meaningful signal is the combination, not any single sub-condition. Identify which sub-conditions contributed.
8. **Schedule sanity check.** If the alert definition has a schedule, confirm the trigger happened inside intended hours. A fire outside intended hours is often a config smell.

Conclude with one of: **real incident** (with the supporting numbers), **noisy / misconfigured query** (with the specific issue — bad threshold, wrong filter, missing schedule), **infrastructure / data-pipeline issue** (NO_DATA cases), or **needs more info** (state when evidence is genuinely thin — don't bluff).

---

### Summarizing comments

When the user asks to summarize comments on a case:
1. Read the `comment` case events in the `get_alerts_object` output for the case.
2. For each comment, capture: actor, timestamp, and `comment_text`.
3. Produce a chronological summary **organized by topic**, not speaker. Call out: hypotheses raised, mitigations attempted, who acknowledged what, and any pointers to external tickets / PR URLs / dashboards in the text. Quote URLs verbatim — never invent them.
4. If a comment is from slack, note that the discussion is mirrored from a Slack thread — the captured comments may be a subset of the live thread.
