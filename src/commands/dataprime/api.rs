use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::error::Result;
use crate::Tier;

use crate::api_client::CxClient;

// ── Log types ─────────────────────────────────────────────────────────────────

/// A parsed log record ready for display or JSON serialisation.
#[derive(Debug, Serialize, Deserialize)]
pub struct LogRecord {
    pub timestamp: Option<String>,
    pub severity: Option<String>,
    pub text: Option<Value>,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

// ── Span / trace types ────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct Span {
    pub trace_id: String,
    pub span_id: String,
    pub operation_name: String,
    pub duration: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Trace {
    pub trace_id: String,
    pub spans: Vec<Span>,
}

/// Generic query response used by `cx dataprime query` and the shared
/// execute/merge/render pipeline.  Source-agnostic — no log- or span-specific
/// fields.
pub struct QueryGenericResponse {
    pub raw_results: Vec<Value>,
    pub warnings: Vec<String>,
    pub is_aggregate: bool,
}

// ── API ───────────────────────────────────────────────────────────────────────

pub struct DataprimeApi<'a> {
    client: &'a CxClient,
}

impl<'a> DataprimeApi<'a> {
    pub fn new(client: &'a CxClient) -> Self {
        Self { client }
    }

    /// POST to `/api/v1/dataprime/query` and return the raw result rows from
    /// the NDJSON response.
    ///
    /// `source` becomes the `defaultSource` metadata field, which Dataprime
    /// uses when the query does not include an explicit `source` command.
    /// Common values: `"logs"`, `"spans"`.
    async fn post_query(
        &self,
        query: &str,
        start_time: &str,
        end_time: &str,
        limit: u32,
        tier: Tier,
        source: &str,
    ) -> Result<(Vec<Value>, Vec<String>)> {
        let body = build_dataprime_body(query, start_time, end_time, limit, tier, source);
        let raw = self
            .client
            .post_raw("/api/v1/dataprime/query", &body)
            .await?;
        parse_ndjson_response(&raw)
    }

    /// Execute a generic Dataprime query as-is, with no default source.
    ///
    /// The query must include its own `source` command (e.g. `source logs | ...`).
    /// Used by `cx dataprime query` and as the shared foundation for logs/spans.
    pub async fn query_generic(
        &self,
        query: &str,
        start_time: &str,
        end_time: &str,
        limit: u32,
        tier: Tier,
        source: &str,
    ) -> Result<QueryGenericResponse> {
        let (rows, warnings) = self
            .post_query(query, start_time, end_time, limit, tier, source)
            .await?;
        let aggregate = is_aggregation_query(query);
        let raw_results = if aggregate {
            rows.iter().map(normalize_aggregate_row).collect()
        } else {
            rows.iter().map(normalize_row).collect()
        };
        Ok(QueryGenericResponse {
            raw_results,
            warnings,
            is_aggregate: aggregate,
        })
    }
}

// ── Request building ──────────────────────────────────────────────────────────

/// Build the JSON request body for a Dataprime query.
pub fn build_dataprime_body(
    query: &str,
    start_time: &str,
    end_time: &str,
    limit: u32,
    tier: Tier,
    source: &str,
) -> Value {
    json!({
        "query": query,
        "metadata": {
            "tier": tier.as_api_str(),
            "syntax": "QUERY_SYNTAX_DATAPRIME",
            "startDate": start_time,
            "endDate": end_time,
            "defaultSource": source,
            "limit": limit
        }
    })
}

// ── NDJSON parsing ────────────────────────────────────────────────────────────

/// Parse a raw NDJSON string from the Dataprime endpoint into result rows and
/// warning messages.
///
/// The Dataprime endpoint returns NDJSON:
/// - line 1: `{"queryId": {...}}`
/// - line 2+: `{"result": {"results": [...]}}` (one or more batches)
/// - optional: `{"warning": {"compileWarning": {"warningMessage": "..."}}}` lines
pub fn parse_ndjson_response(raw: &str) -> Result<(Vec<Value>, Vec<String>)> {
    let mut rows: Vec<Value> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let value: Value = serde_json::from_str(line)?;
        if let Some(batch) = value
            .get("result")
            .and_then(|r| r.get("results"))
            .and_then(|r| r.as_array())
        {
            rows.extend(batch.iter().cloned());
        } else if let Some(w) = value.get("warning").and_then(|v| v.as_object()) {
            for inner in w.values() {
                if let Some(msg) = inner.get("warningMessage").and_then(|m| m.as_str()) {
                    warnings.push(msg.to_string());
                }
            }
        }
    }
    Ok((rows, warnings))
}

// ── Aggregate-query detection ─────────────────────────────────────────────────

/// Return `true` when the query contains an aggregating command.
///
/// The aggregating commands are `aggregate`, `groupby`, `multigroupby`,
/// `count`, `countby`, and `distinct`.  A command is recognised when it is
/// the first token of the query string or the first token of a pipe-separated
/// segment (i.e. it appears immediately after a `|`).  The check is
/// case-insensitive.
pub fn is_aggregation_query(query: &str) -> bool {
    const AGG_COMMANDS: &[&str] = &[
        "aggregate",
        "groupby",
        "multigroupby",
        "count",
        "countby",
        "distinct",
    ];

    for segment in query.split('|') {
        let first = segment.split_whitespace().next().unwrap_or("");
        if AGG_COMMANDS.contains(&first.to_lowercase().as_str()) {
            return true;
        }
    }
    false
}

// ── Row normalization ─────────────────────────────────────────────────────────

/// Normalize a raw Dataprime result row for output:
///
/// - `metadata` `[{key, value}]` → `{key: value}` object
/// - `labels`   `[{key, value}]` → `{key: value}` object
/// - `userData` JSON-encoded string → parsed JSON value (falls back to the
///   original string if it cannot be parsed)
pub fn normalize_row(record: &Value) -> Value {
    let mut out = match record {
        Value::Object(m) => m.clone(),
        _ => return record.clone(),
    };

    for field in ["metadata", "labels"] {
        if let Some(arr) = out.get(field).and_then(|v| v.as_array()) {
            let map: serde_json::Map<String, Value> = arr
                .iter()
                .filter_map(|item| {
                    let k = item.get("key")?.as_str()?.to_string();
                    let v = item.get("value")?.clone();
                    Some((k, v))
                })
                .collect();
            out.insert(field.to_string(), Value::Object(map));
        }
    }

    if let Some(raw) = out.get("userData").and_then(|v| v.as_str()) {
        if let Ok(parsed) = serde_json::from_str::<Value>(raw) {
            out.insert("userData".to_string(), parsed);
        }
    }

    Value::Object(out)
}

/// Normalize a raw Dataprime result row from an aggregation query.
///
/// Aggregate rows always have empty `metadata` and `labels`.  The only
/// interesting content is in `userData`, which is a JSON-encoded string
/// containing the aggregated fields (e.g. `{"region":"us1","total_logs":16}`).
/// This function extracts and parses that string and returns the inner object
/// directly, discarding the empty envelope fields.
///
/// If `userData` cannot be parsed as JSON the original string value is
/// returned as-is.
pub fn normalize_aggregate_row(record: &Value) -> Value {
    let raw = record
        .get("userData")
        .and_then(|v| v.as_str())
        .unwrap_or("{}");
    serde_json::from_str::<Value>(raw).unwrap_or_else(|_| Value::String(raw.to_string()))
}

// ── Log record parsing ────────────────────────────────────────────────────────

/// Convert a single normalized result row into a `LogRecord`.
///
/// Expects rows that have already been passed through `normalize_row`:
/// `metadata` and `labels` are objects, and `userData` is a parsed JSON value.
pub fn parse_log_record(record: &Value) -> LogRecord {
    let user_data: Value = record.get("userData").cloned().unwrap_or(Value::Null);

    let timestamp = extract_field(
        &user_data,
        &[
            "$m.timestamp",
            "coralogix.timestamp",
            "timestamp",
            "@timestamp",
        ],
    );

    // Severity is always in the required metadata.severity field.
    let severity = extract_severity(record.get("metadata").unwrap_or(&Value::Null));

    let extra: HashMap<String, Value> = match &user_data {
        Value::Object(map) => map.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        _ => HashMap::new(),
    };

    LogRecord {
        timestamp,
        severity,
        text: if user_data.is_null() {
            None
        } else {
            Some(user_data)
        },
        extra,
    }
}

/// Extract and normalize severity from a normalized metadata object.
///
/// Expects the `metadata` value after `normalize_row` has been applied,
/// i.e. an object with a `"severity"` key.
/// Maps Coralogix numeric codes to human-readable strings.
pub fn extract_severity(metadata: &Value) -> Option<String> {
    let raw = metadata.get("severity").and_then(|v| v.as_str())?;
    // Coralogix numeric severity: 1=DEBUG 2=VERBOSE 3=INFO 4=WARNING 5=ERROR 6=CRITICAL
    Some(match raw {
        "1" => "DEBUG".to_string(),
        "2" => "VERBOSE".to_string(),
        "3" => "INFO".to_string(),
        "4" => "WARNING".to_string(),
        "5" => "ERROR".to_string(),
        "6" => "CRITICAL".to_string(),
        other => other.to_uppercase(),
    })
}

// ── Span record parsing ───────────────────────────────────────────────────────

/// Parse a single normalized Dataprime result row into a `Span`.
///
/// Expects rows that have already been passed through `normalize_row`, so
/// `userData` is a parsed JSON value rather than a JSON-encoded string.
/// Falls back to the raw record itself so the function also handles
/// pre-parsed/flat JSON objects in tests.
pub fn parse_span_record(record: &Value) -> Option<Span> {
    let user_data: Value = record
        .get("userData")
        .cloned()
        .unwrap_or_else(|| record.clone());

    let trace_id = extract_field(&user_data, &["traceID", "trace_id", "$d.traceID"])?;
    let span_id = extract_field(&user_data, &["spanID", "spanId", "span_id", "$d.spanId"])?;
    let operation_name = extract_field(
        &user_data,
        &["operationName", "operation_name", "$l.operationName"],
    )
    .unwrap_or_default();
    let duration = extract_field(&user_data, &["duration", "$m.duration"])
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);

    Some(Span {
        trace_id,
        span_id,
        operation_name,
        duration,
    })
}

/// Group a flat list of span rows into traces, preserving the insertion order
/// of each trace's first occurrence.
pub fn group_spans_into_traces(rows: Vec<Value>) -> Vec<Trace> {
    let mut map: HashMap<String, Vec<Span>> = HashMap::new();
    let mut order: Vec<String> = Vec::new();

    for row in &rows {
        if let Some(span) = parse_span_record(row) {
            let tid = span.trace_id.clone();
            if !map.contains_key(&tid) {
                order.push(tid.clone());
            }
            map.entry(tid).or_default().push(span);
        }
    }

    order
        .into_iter()
        .map(|tid| {
            let spans = map.remove(&tid).unwrap_or_default();
            Trace {
                trace_id: tid,
                spans,
            }
        })
        .collect()
}

// ── Shared field extraction ───────────────────────────────────────────────────

/// Walk a dot-separated path (e.g. `"$m.timestamp"`) through a JSON value and
/// return the first one that yields a non-null result.
pub fn extract_field(v: &Value, paths: &[&str]) -> Option<String> {
    for path in paths {
        let mut current = v;
        let mut ok = true;
        for segment in path.split('.') {
            match current.get(segment) {
                Some(next) => current = next,
                None => {
                    ok = false;
                    break;
                }
            }
        }
        if ok && !current.is_null() {
            return Some(match current {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            });
        }
    }
    None
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn kv_array(pairs: &[(&str, &str)]) -> Value {
        Value::Array(
            pairs
                .iter()
                .map(|(k, v)| json!({"key": k, "value": v}))
                .collect(),
        )
    }

    #[test]
    fn normalize_metadata_list_to_object() {
        let row = json!({
            "metadata": kv_array(&[("severity", "Error"), ("logid", "abc123")]),
            "labels": [],
            "userData": "{}"
        });
        let out = normalize_row(&row);
        assert_eq!(out["metadata"]["severity"], json!("Error"));
        assert_eq!(out["metadata"]["logid"], json!("abc123"));
    }

    #[test]
    fn normalize_labels_list_to_object() {
        let row = json!({
            "metadata": [],
            "labels": kv_array(&[("applicationname", "api"), ("subsystemname", "us1")]),
            "userData": "{}"
        });
        let out = normalize_row(&row);
        assert_eq!(out["labels"]["applicationname"], json!("api"));
        assert_eq!(out["labels"]["subsystemname"], json!("us1"));
    }

    #[test]
    fn normalize_user_data_string_to_json() {
        let row = json!({
            "metadata": [],
            "labels": [],
            "userData": r#"{"message":"hello","level":"INFO"}"#
        });
        let out = normalize_row(&row);
        assert_eq!(out["userData"]["message"], json!("hello"));
        assert_eq!(out["userData"]["level"], json!("INFO"));
    }

    #[test]
    fn normalize_user_data_non_json_stays_as_string() {
        let raw_str = "plain text log line";
        let row = json!({
            "metadata": [],
            "labels": [],
            "userData": raw_str
        });
        let out = normalize_row(&row);
        assert_eq!(out["userData"], json!(raw_str));
    }

    #[test]
    fn normalize_full_record_matches_expected_cli_output() {
        let row = json!({
            "metadata": kv_array(&[
                ("severity", "Error"),
                ("timestamp", "2026-03-21T09:05:47.879644")
            ]),
            "labels": kv_array(&[
                ("applicationname", "api"),
                ("subsystemname", "us3")
            ]),
            "userData": r#"{"message":"Health check timeout","levelname":"ERROR"}"#
        });
        let out = normalize_row(&row);
        assert!(out["metadata"].is_object());
        assert!(out["labels"].is_object());
        assert!(out["userData"].is_object());
        assert_eq!(out["metadata"]["severity"], json!("Error"));
        assert_eq!(out["labels"]["applicationname"], json!("api"));
        assert_eq!(out["userData"]["message"], json!("Health check timeout"));
    }

    // ── is_aggregation_query ──────────────────────────────────────────────────

    #[test]
    fn agg_query_groupby_after_pipe() {
        assert!(is_aggregation_query(
            "source logs | groupby $l.subsystemname as region aggregate count() as total_logs"
        ));
    }

    #[test]
    fn agg_query_aggregate_after_pipe() {
        assert!(is_aggregation_query("source logs | aggregate count()"));
    }

    #[test]
    fn agg_query_count_after_pipe() {
        assert!(is_aggregation_query("source logs | count"));
    }

    #[test]
    fn agg_query_countby_after_pipe() {
        assert!(is_aggregation_query("source logs | countby $l.app"));
    }

    #[test]
    fn agg_query_multigroupby_after_pipe() {
        assert!(is_aggregation_query(
            "source logs | multigroupby a, b aggregate count()"
        ));
    }

    #[test]
    fn agg_query_distinct_after_pipe() {
        assert!(is_aggregation_query("source logs | distinct $d.field"));
    }

    #[test]
    fn agg_query_keyword_at_start_without_source() {
        assert!(is_aggregation_query("groupby $l.app aggregate count()"));
    }

    #[test]
    fn agg_query_keyword_case_insensitive() {
        assert!(is_aggregation_query(
            "source logs | GroupBy $l.app aggregate count()"
        ));
    }

    #[test]
    fn non_agg_query_filter_is_not_aggregate() {
        assert!(!is_aggregation_query(
            r#"source logs | filter $d.message ~= "timeout""#
        ));
    }

    #[test]
    fn non_agg_query_source_only() {
        assert!(!is_aggregation_query("source logs"));
    }

    #[test]
    fn non_agg_query_word_containing_count_in_value_is_not_aggregate() {
        // "countby" must be at a command position (first token of a segment)
        assert!(!is_aggregation_query(
            r#"source logs | filter $d.label == "countby-region""#
        ));
    }

    // ── normalize_aggregate_row ───────────────────────────────────────────────

    #[test]
    fn aggregate_row_extracts_user_data() {
        let row = json!({
            "metadata": [],
            "labels": [],
            "userData": r#"{"region":"us1","total_logs":16}"#
        });
        let out = normalize_aggregate_row(&row);
        assert_eq!(out["region"], json!("us1"));
        assert_eq!(out["total_logs"], json!(16));
        assert!(out.get("metadata").is_none());
        assert!(out.get("labels").is_none());
    }

    #[test]
    fn aggregate_row_non_json_user_data_kept_as_string() {
        let row = json!({"metadata": [], "labels": [], "userData": "not json"});
        let out = normalize_aggregate_row(&row);
        assert_eq!(out, json!("not json"));
    }

    #[test]
    fn aggregate_rows_from_groupby_example() {
        // Mirror of the example provided in the task description
        let rows = [
            json!({"metadata": [], "labels": [], "userData": r#"{"region":"us1","total_logs":16}"#}),
            json!({"metadata": [], "labels": [], "userData": r#"{"region":"us2","total_logs":20}"#}),
            json!({"metadata": [], "labels": [], "userData": r#"{"region":"us3","total_logs":12}"#}),
        ];
        let results: Vec<_> = rows.iter().map(normalize_aggregate_row).collect();
        assert_eq!(results[0]["region"], json!("us1"));
        assert_eq!(results[1]["total_logs"], json!(20));
        assert_eq!(results[2]["region"], json!("us3"));
        // Envelope fields must not appear in the output
        for r in &results {
            assert!(r.get("metadata").is_none());
            assert!(r.get("labels").is_none());
            assert!(r.get("userData").is_none());
        }
    }
}
