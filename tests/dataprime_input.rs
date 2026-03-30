/// Tests that verify commands translate to the correct Dataprime API call structure.
use cx::api::dataprime::build_dataprime_body;
use cx::commands::spans::normalize_query;
use cx::Tier;

// ── Request body construction ─────────────────────────────────────────────────

#[test]
fn build_body_logs_source() {
    let body = build_dataprime_body("*", "2026-01-01T00:00:00Z", "2026-01-02T00:00:00Z", 100, Tier::FrequentSearch, "logs");
    assert_eq!(body["metadata"]["defaultSource"], "logs");
}

#[test]
fn build_body_spans_source() {
    let body = build_dataprime_body("*", "2026-01-01T00:00:00Z", "2026-01-02T00:00:00Z", 100, Tier::FrequentSearch, "spans");
    assert_eq!(body["metadata"]["defaultSource"], "spans");
}

#[test]
fn build_body_tier_frequent() {
    let body = build_dataprime_body("*", "2026-01-01T00:00:00Z", "2026-01-02T00:00:00Z", 100, Tier::FrequentSearch, "logs");
    assert_eq!(body["metadata"]["tier"], "TIER_FREQUENT_SEARCH");
}

#[test]
fn build_body_tier_archive() {
    let body = build_dataprime_body("*", "2026-01-01T00:00:00Z", "2026-01-02T00:00:00Z", 100, Tier::Archive, "logs");
    assert_eq!(body["metadata"]["tier"], "TIER_ARCHIVE");
}

#[test]
fn build_body_syntax_is_dataprime() {
    let body = build_dataprime_body("*", "2026-01-01T00:00:00Z", "2026-01-02T00:00:00Z", 100, Tier::FrequentSearch, "logs");
    assert_eq!(body["metadata"]["syntax"], "QUERY_SYNTAX_DATAPRIME");
}

#[test]
fn build_body_all_metadata_fields_present() {
    let body = build_dataprime_body(
        "filter $d.level == \"ERROR\"",
        "2026-01-01T00:00:00Z",
        "2026-01-02T00:00:00Z",
        50,
        Tier::FrequentSearch,
        "logs",
    );
    assert_eq!(body["query"], "filter $d.level == \"ERROR\"");
    assert_eq!(body["metadata"]["startDate"], "2026-01-01T00:00:00Z");
    assert_eq!(body["metadata"]["endDate"], "2026-01-02T00:00:00Z");
    assert_eq!(body["metadata"]["limit"], 50);
}

// ── Spans query normalization ─────────────────────────────────────────────────

#[test]
fn normalize_query_prepends_source_spans_when_missing() {
    let q = normalize_query("filter $d.traceID == \"abc123\"");
    assert!(q.starts_with("source spans |"), "query should start with 'source spans |', got: {q}");
}

#[test]
fn normalize_query_preserves_existing_source_spans() {
    let q = normalize_query("source spans | filter $d.traceID == \"abc123\"");
    assert_eq!(q, "source spans | filter $d.traceID == \"abc123\"");
}

#[test]
fn normalize_query_preserves_different_source() {
    let q = normalize_query("source logs | filter $d.level == \"ERROR\"");
    assert!(q.starts_with("source logs"), "query with different source should be preserved, got: {q}");
}

#[test]
fn normalize_query_handles_case_insensitive_source() {
    let q = normalize_query("SOURCE spans | filter $d.error == true");
    assert_eq!(q, "SOURCE spans | filter $d.error == true");
}

#[test]
fn normalize_query_trims_leading_whitespace() {
    let q = normalize_query("  filter $d.traceID == \"abc\"  ");
    assert!(q.starts_with("source spans |"), "query should be trimmed and normalized, got: {q}");
}
