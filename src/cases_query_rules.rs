//! Hard-rule validation for fleet case analytics DataPrime queries.
//!
//! Port of olly's `cases_query_rules.py`.

use chrono::{DateTime, Utc};
use regex::Regex;
use std::sync::LazyLock;

pub const CASES_DATASET_SOURCE: &str = "system/labs.cases.state_updates";

const MIN_CASES_WINDOW_SECS: i64 = 3600;

static INLINE_SOURCE_RE: LazyLock<Regex> = LazyLock::new(|| {
    // Matches the DataPrime `source <dataset>` command at the start of a query
    // or immediately after a pipe `|`, capturing the dataset name.
    Regex::new(r"(?i)(?:^|\|)\s*source\s+([\w/\.]+)").unwrap()
});

static CASES_SAFE_PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    let case_id_ref = r"(?:\$d\.|userdata\.)?caseid";
    vec![
        Regex::new(&format!(r"(?i)\bgroupby\s+{case_id_ref}\b")).unwrap(),
        Regex::new(&format!(r"(?i)\bdedupeby\s+{case_id_ref}\b")).unwrap(),
        Regex::new(&format!(r"(?i)\bdistinct\s+{case_id_ref}\b")).unwrap(),
        Regex::new(r"(?i)\bfilter\b[^|]*\bmetadata\.trigger\b").unwrap(),
    ]
});

/// Parse an API-formatted timestamp (`2006-01-02T15:04:05.000Z`) into UTC.
fn parse_api_timestamp(ts: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(ts)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

/// Check hard rules for queries against `system/labs.cases.state_updates`.
///
/// Returns an actionable warning string, or `None` when everything looks fine.
pub fn check_cases_query_rules(
    source: &str,
    query: &str,
    start_ts: &str,
    end_ts: &str,
) -> Option<String> {
    let effective_source = if source == CASES_DATASET_SOURCE {
        source.to_string()
    } else {
        INLINE_SOURCE_RE
            .captures(query)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().to_string())
            .unwrap_or_default()
    };
    if effective_source != CASES_DATASET_SOURCE {
        return None;
    }

    let mut violations: Vec<&str> = Vec::new();

    // Rule 1 — minimum 1h window
    if let (Some(start), Some(end)) = (parse_api_timestamp(start_ts), parse_api_timestamp(end_ts)) {
        let window_secs = (end - start).num_seconds();
        if window_secs < MIN_CASES_WINDOW_SECS {
            violations.push(
                "Rule 1 (time range): the time window is shorter than the 1h minimum \
                 required for `system/labs.cases.state_updates`. \
                 Widen the window to at least `1h`.",
            );
        }
    }

    // Rule 2/3 — per-case dedup or lifecycle-trigger filter
    if !CASES_SAFE_PATTERNS.iter().any(|re| re.is_match(query)) {
        violations.push(
            "Rule 2/3 (dedup): no per-case dedup or lifecycle trigger filter detected. \
             Add one of: \
             `| dedupeby caseId orderby $m.timestamp desc`, \
             `| groupby caseId aggregate ...`, \
             `| distinct caseId`, \
             or `| filter metadata.trigger == '...'`.",
        );
    }

    if violations.is_empty() {
        return None;
    }

    let mut lines = vec!["\n\n> **[Cases query warning]** This query targets \
         `system/labs.cases.state_updates` but may violate hard rules:"
        .to_string()];
    for violation in violations {
        lines.push(format!("> - {violation}"));
    }
    Some(lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const START: &str = "2024-01-01T00:00:00.000Z";
    const END: &str = "2024-01-01T02:00:00.000Z";

    #[test]
    fn non_cases_source_returns_none() {
        assert!(check_cases_query_rules("logs", "source logs | count", START, END).is_none());
        assert!(check_cases_query_rules("", "source logs | count", START, END).is_none());
    }

    #[test]
    fn inline_source_in_query_triggers_rules() {
        let end = "2024-01-01T00:30:00.000Z";
        let warning = check_cases_query_rules(
            "",
            "source system/labs.cases.state_updates | count",
            START,
            end,
        )
        .unwrap();
        assert!(warning.contains("Rule 1"));
        assert!(warning.contains("Rule 2/3"));
    }

    #[test]
    fn short_window_triggers_rule1() {
        let end = "2024-01-01T00:30:00.000Z";
        let warning = check_cases_query_rules(CASES_DATASET_SOURCE, "| count", START, end).unwrap();
        assert!(warning.contains("Rule 1"));
    }

    #[test]
    fn missing_dedup_triggers_rule2() {
        let warning = check_cases_query_rules(
            CASES_DATASET_SOURCE,
            "source system/labs.cases.state_updates | count",
            START,
            END,
        )
        .unwrap();
        assert!(warning.contains("Rule 2/3"));
    }

    #[test]
    fn dedupeby_caseid_is_safe() {
        assert!(check_cases_query_rules(
            CASES_DATASET_SOURCE,
            "| dedupeby caseId orderby $m.timestamp desc",
            START,
            END
        )
        .is_none());
    }

    #[test]
    fn trigger_filter_is_safe() {
        assert!(check_cases_query_rules(
            CASES_DATASET_SOURCE,
            "| filter metadata.trigger == 'caseResolved'",
            START,
            END
        )
        .is_none());
    }
}
