/// Tests that verify Dataprime API responses are correctly parsed into typed
/// records and formatted output.
use cx::commands::dataprime::api::{
    extract_severity, group_spans_into_traces, is_aggregation_query, normalize_aggregate_row,
    normalize_row, parse_log_record, parse_ndjson_response, parse_span_record,
};
use serde_json::{json, Value};

// ── NDJSON response parsing ───────────────────────────────────────────────────

fn kv_array(pairs: &[(&str, &str)]) -> Value {
    Value::Array(
        pairs
            .iter()
            .map(|(k, v)| json!({"key": k, "value": v}))
            .collect(),
    )
}

/// Wrap a list of result rows in the Dataprime NDJSON envelope and serialize
/// as proper NDJSON (one JSON object per line).
fn make_ndjson(rows: &[Value]) -> String {
    let header = serde_json::to_string(&json!({"queryId": {"queryId": "test-id"}})).unwrap();
    let result = serde_json::to_string(&json!({"result": {"results": rows}})).unwrap();
    format!("{header}\n{result}\n")
}

#[test]
fn ndjson_skips_query_id_line() {
    let ndjson = r#"{"queryId":{"queryId":"abc"}}
"#;
    let (rows, warnings) = parse_ndjson_response(ndjson).unwrap();
    assert!(rows.is_empty());
    assert!(warnings.is_empty());
}

#[test]
fn ndjson_extracts_result_rows() {
    let ndjson = make_ndjson(&[json!({"userData": "{}"}), json!({"userData": "{}"})]);
    let (rows, _) = parse_ndjson_response(&ndjson).unwrap();
    assert_eq!(rows.len(), 2);
}

#[test]
fn ndjson_extracts_warning_message() {
    let ndjson = r#"{"queryId":{"queryId":"abc"}}
{"warning":{"compileWarning":{"warningMessage":"index not found for field xyz"}}}
"#;
    let (rows, warnings) = parse_ndjson_response(ndjson).unwrap();
    assert!(rows.is_empty());
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("index not found"));
}

#[test]
fn ndjson_ignores_empty_lines() {
    let ndjson = "\n\n\n";
    let (rows, warnings) = parse_ndjson_response(ndjson).unwrap();
    assert!(rows.is_empty());
    assert!(warnings.is_empty());
}

#[test]
fn ndjson_handles_multiple_result_batches() {
    let header = serde_json::to_string(&json!({"queryId": {"queryId": "x"}})).unwrap();
    let batch1 = serde_json::to_string(&json!({"result": {"results": [json!({"a": 1})]}})).unwrap();
    let batch2 =
        serde_json::to_string(&json!({"result": {"results": [json!({"a": 2}), json!({"a": 3})]}}))
            .unwrap();
    let ndjson = format!("{header}\n{batch1}\n{batch2}\n");
    let (rows, _) = parse_ndjson_response(&ndjson).unwrap();
    assert_eq!(rows.len(), 3);
}

#[test]
fn ndjson_from_example_logs_data() {
    // 5 log rows taken from example_dataprime_logs_response.json
    let rows = vec![
        json!({"metadata": kv_array(&[("severity","Warning"),("timestamp","2026-03-21T09:34:56.062881")]), "labels": kv_array(&[("applicationname","api")]), "userData": "{\"levelname\":\"WARNING\",\"message\":\"OPENAI_API_KEY is not set, skipping trace export\"}"}),
        json!({"metadata": kv_array(&[("severity","Warning"),("timestamp","2026-03-21T09:35:01.065767")]), "labels": kv_array(&[("applicationname","api")]), "userData": "{\"levelname\":\"WARNING\",\"message\":\"OPENAI_API_KEY is not set, skipping trace export\"}"}),
        json!({"metadata": kv_array(&[("severity","Info"),  ("timestamp","2026-03-21T09:35:20.378890")]), "labels": kv_array(&[("applicationname","api")]), "userData": "{\"levelname\":\"INFO\",\"message\":\"Authentication failed: AuthErrorReason.SESSION_TOKEN_MISSING\"}"}),
        json!({"metadata": kv_array(&[("severity","Info"),  ("timestamp","2026-03-21T09:40:11.817193")]), "labels": kv_array(&[("applicationname","api")]), "userData": "{\"text\":\"The Application Name api and Subsystem Name ap2 from the Python SDK\"}"}),
        json!({"metadata": kv_array(&[("severity","Error"), ("timestamp","2026-03-21T09:39:51.294912")]), "labels": kv_array(&[("applicationname","api")]), "userData": "{\"levelname\":\"ERROR\",\"message\":\"Health check failure\"}"}),
    ];
    let ndjson = make_ndjson(&rows);
    let (parsed_rows, warnings) = parse_ndjson_response(&ndjson).unwrap();
    assert_eq!(parsed_rows.len(), 5);
    assert!(warnings.is_empty());
}

#[test]
fn ndjson_from_example_traces_data() {
    // 5 span rows taken from example_dataprime_traces_response.json
    let rows = vec![
        json!({"metadata": kv_array(&[("duration","26142")]), "labels": kv_array(&[("operationName","PING"),("serviceName","api")]), "userData": "{\"spanID\":\"0cd3201c4b783729\",\"traceID\":\"7e11b3be4f2f57a1d3f97648d7f59b64\",\"operationName\":\"PING\",\"duration\":26142}"}),
        json!({"metadata": kv_array(&[("duration","10383")]), "labels": kv_array(&[("operationName","GET"), ("serviceName","api")]), "userData": "{\"spanID\":\"d3ab341e5239f7f0\",\"traceID\":\"7e11b3be4f2f57a1d3f97648d7f59b64\",\"operationName\":\"GET\",\"duration\":10383}"}),
        json!({"metadata": kv_array(&[("duration","26502")]), "labels": kv_array(&[("operationName","GET"), ("serviceName","api")]), "userData": "{\"spanID\":\"e3ffaa51524b69ab\",\"traceID\":\"7e11b3be4f2f57a1d3f97648d7f59b64\",\"operationName\":\"GET\",\"duration\":26502}"}),
        json!({"metadata": kv_array(&[("duration","44965")]), "labels": kv_array(&[("operationName","GET"), ("serviceName","api")]), "userData": "{\"spanID\":\"36a2c1d7bc45bbd4\",\"traceID\":\"7e11b3be4f2f57a1d3f97648d7f59b64\",\"operationName\":\"GET\",\"duration\":44965}"}),
        json!({"metadata": kv_array(&[("duration","35")]),    "labels": kv_array(&[("operationName","GET /health/readiness http send"),("serviceName","api")]), "userData": "{\"spanID\":\"cb379f67b0f45e9e\",\"traceID\":\"7e11b3be4f2f57a1d3f97648d7f59b64\",\"operationName\":\"GET /health/readiness http send\",\"duration\":35}"}),
    ];
    let ndjson = make_ndjson(&rows);
    let (parsed_rows, warnings) = parse_ndjson_response(&ndjson).unwrap();
    assert_eq!(parsed_rows.len(), 5);
    assert!(warnings.is_empty());
}

// ── Log record parsing ────────────────────────────────────────────────────────

#[test]
fn parse_log_record_extracts_timestamp() {
    let row = normalize_row(&json!({
        "metadata": [],
        "labels": [],
        "userData": r#"{"timestamp":"2026-03-21T09:34:56.062881","message":"hello"}"#
    }));
    let record = parse_log_record(&row);
    assert_eq!(
        record.timestamp.as_deref(),
        Some("2026-03-21T09:34:56.062881")
    );
}

#[test]
fn parse_log_record_severity_warning_string() {
    let row = normalize_row(&json!({
        "metadata": kv_array(&[("severity", "Warning")]),
        "labels": [],
        "userData": r#"{"message":"test"}"#
    }));
    let record = parse_log_record(&row);
    assert_eq!(record.severity.as_deref(), Some("WARNING"));
}

#[test]
fn parse_log_record_severity_error_string() {
    let row = normalize_row(&json!({
        "metadata": kv_array(&[("severity", "Error")]),
        "labels": [],
        "userData": r#"{"message":"test"}"#
    }));
    let record = parse_log_record(&row);
    assert_eq!(record.severity.as_deref(), Some("ERROR"));
}

#[test]
fn parse_log_record_severity_info_string() {
    let row = normalize_row(&json!({
        "metadata": kv_array(&[("severity", "Info")]),
        "labels": [],
        "userData": r#"{"message":"test"}"#
    }));
    let record = parse_log_record(&row);
    assert_eq!(record.severity.as_deref(), Some("INFO"));
}

#[test]
fn parse_log_record_numeric_severity_5_is_error() {
    let row = normalize_row(&json!({
        "metadata": kv_array(&[("severity", "5")]),
        "labels": [],
        "userData": r#"{"message":"test"}"#
    }));
    let record = parse_log_record(&row);
    assert_eq!(record.severity.as_deref(), Some("ERROR"));
}

#[test]
fn parse_log_record_numeric_severity_4_is_warning() {
    let row = normalize_row(&json!({
        "metadata": kv_array(&[("severity", "4")]),
        "labels": [],
        "userData": r#"{"message":"test"}"#
    }));
    let record = parse_log_record(&row);
    assert_eq!(record.severity.as_deref(), Some("WARNING"));
}

#[test]
fn parse_log_record_numeric_severity_3_is_info() {
    let row = normalize_row(&json!({
        "metadata": kv_array(&[("severity", "3")]),
        "labels": [],
        "userData": r#"{"message":"test"}"#
    }));
    let record = parse_log_record(&row);
    assert_eq!(record.severity.as_deref(), Some("INFO"));
}

#[test]
fn parse_log_record_from_example_warning_row() {
    // Row 1 from example_dataprime_logs_response.json.
    // Severity comes from metadata.severity; userData contains Python's levelname (not used).
    let row = normalize_row(&json!({
        "metadata": kv_array(&[
            ("severity", "Warning"),
            ("timestamp", "2026-03-21T09:34:56.062881")
        ]),
        "labels": kv_array(&[("applicationname", "api"), ("subsystemname", "euprod2")]),
        "userData": "{\"levelname\":\"WARNING\",\"message\":\"OPENAI_API_KEY is not set, skipping trace export\"}"
    }));
    let record = parse_log_record(&row);
    assert_eq!(record.severity.as_deref(), Some("WARNING"));
    assert!(record.text.is_some());
}

#[test]
fn parse_log_record_from_example_error_row() {
    // Row 5 from example_dataprime_logs_response.json (health-check failure).
    // Severity comes from metadata.severity; userData contains Python's levelname (not used).
    let row = normalize_row(&json!({
        "metadata": kv_array(&[
            ("severity", "Error"),
            ("timestamp", "2026-03-21T09:39:51.294912")
        ]),
        "labels": kv_array(&[("applicationname", "api"), ("subsystemname", "ap2")]),
        "userData": "{\"levelname\":\"ERROR\",\"message\":\"Health check failure\",\"error_type\":\"connection_refused\"}"
    }));
    let record = parse_log_record(&row);
    assert_eq!(record.severity.as_deref(), Some("ERROR"));
    assert!(record.text.is_some());
}

// ── Severity extraction ───────────────────────────────────────────────────────

#[test]
fn extract_severity_numeric_1_is_debug() {
    assert_eq!(
        extract_severity(&json!({"severity": "1"})).as_deref(),
        Some("DEBUG")
    );
}

#[test]
fn extract_severity_numeric_2_is_verbose() {
    assert_eq!(
        extract_severity(&json!({"severity": "2"})).as_deref(),
        Some("VERBOSE")
    );
}

#[test]
fn extract_severity_numeric_6_is_critical() {
    assert_eq!(
        extract_severity(&json!({"severity": "6"})).as_deref(),
        Some("CRITICAL")
    );
}

#[test]
fn extract_severity_unknown_string_is_uppercased() {
    assert_eq!(
        extract_severity(&json!({"severity": "trace"})).as_deref(),
        Some("TRACE")
    );
}

#[test]
fn extract_severity_missing_returns_none() {
    assert_eq!(extract_severity(&json!({"message": "hello"})), None);
}

// ── Span/Trace parsing ────────────────────────────────────────────────────────

#[test]
fn parse_span_extracts_trace_id_and_span_id() {
    let row = json!({
        "traceID": "abc123",
        "spanID": "span456",
        "operationName": "GET",
        "duration": 1000
    });
    let span = parse_span_record(&row).unwrap();
    assert_eq!(span.trace_id, "abc123");
    assert_eq!(span.span_id, "span456");
}

#[test]
fn parse_span_extracts_operation_and_duration() {
    let row = json!({
        "traceID": "abc",
        "spanID": "span1",
        "operationName": "POST /api/chat",
        "duration": 42000
    });
    let span = parse_span_record(&row).unwrap();
    assert_eq!(span.operation_name, "POST /api/chat");
    assert_eq!(span.duration, 42000);
}

#[test]
fn parse_span_returns_none_when_trace_id_missing() {
    let row = json!({"spanID": "span1", "operationName": "GET", "duration": 100});
    assert!(parse_span_record(&row).is_none());
}

#[test]
fn group_spans_same_trace_id_yields_one_trace() {
    let rows = vec![
        json!({"traceID": "trace1", "spanID": "span1", "operationName": "A", "duration": 100}),
        json!({"traceID": "trace1", "spanID": "span2", "operationName": "B", "duration": 200}),
    ];
    let traces = group_spans_into_traces(rows);
    assert_eq!(traces.len(), 1);
    assert_eq!(traces[0].trace_id, "trace1");
    assert_eq!(traces[0].spans.len(), 2);
}

#[test]
fn group_spans_multiple_trace_ids() {
    let rows = vec![
        json!({"traceID": "trace1", "spanID": "span1", "operationName": "A", "duration": 10}),
        json!({"traceID": "trace2", "spanID": "span2", "operationName": "B", "duration": 20}),
        json!({"traceID": "trace1", "spanID": "span3", "operationName": "C", "duration": 30}),
    ];
    let traces = group_spans_into_traces(rows);
    assert_eq!(traces.len(), 2);
    let t1 = traces.iter().find(|t| t.trace_id == "trace1").unwrap();
    assert_eq!(t1.spans.len(), 2);
    let t2 = traces.iter().find(|t| t.trace_id == "trace2").unwrap();
    assert_eq!(t2.spans.len(), 1);
}

#[test]
fn group_spans_preserves_insertion_order() {
    let rows = vec![
        json!({"traceID": "beta",  "spanID": "s1", "operationName": "X", "duration": 1}),
        json!({"traceID": "alpha", "spanID": "s2", "operationName": "Y", "duration": 2}),
        json!({"traceID": "gamma", "spanID": "s3", "operationName": "Z", "duration": 3}),
    ];
    let traces = group_spans_into_traces(rows);
    assert_eq!(traces[0].trace_id, "beta");
    assert_eq!(traces[1].trace_id, "alpha");
    assert_eq!(traces[2].trace_id, "gamma");
}

#[test]
fn group_spans_from_example_traces() {
    // All 5 spans from example_dataprime_traces_response.json - all belong to one trace
    let rows = vec![
        json!({"traceID": "7e11b3be4f2f57a1d3f97648d7f59b64", "spanID": "0cd3201c4b783729", "operationName": "PING",                          "duration": 26142}),
        json!({"traceID": "7e11b3be4f2f57a1d3f97648d7f59b64", "spanID": "d3ab341e5239f7f0", "operationName": "GET",                           "duration": 10383}),
        json!({"traceID": "7e11b3be4f2f57a1d3f97648d7f59b64", "spanID": "e3ffaa51524b69ab", "operationName": "GET",                           "duration": 26502}),
        json!({"traceID": "7e11b3be4f2f57a1d3f97648d7f59b64", "spanID": "36a2c1d7bc45bbd4", "operationName": "GET",                           "duration": 44965}),
        json!({"traceID": "7e11b3be4f2f57a1d3f97648d7f59b64", "spanID": "cb379f67b0f45e9e", "operationName": "GET /health/readiness http send","duration": 35}),
    ];
    let traces = group_spans_into_traces(rows);
    assert_eq!(traces.len(), 1, "all 5 spans share one trace ID");
    let trace = &traces[0];
    assert_eq!(trace.trace_id, "7e11b3be4f2f57a1d3f97648d7f59b64");
    assert_eq!(trace.spans.len(), 5);

    // Verify the PING span details
    let ping = trace
        .spans
        .iter()
        .find(|s| s.operation_name == "PING")
        .unwrap();
    assert_eq!(ping.span_id, "0cd3201c4b783729");
    assert_eq!(ping.duration, 26142);

    // Verify the shortest span (GET /health/readiness http send, 35µs)
    let short = trace
        .spans
        .iter()
        .find(|s| s.span_id == "cb379f67b0f45e9e")
        .unwrap();
    assert_eq!(short.duration, 35);
}

// ── Aggregate query detection ─────────────────────────────────────────────────

#[test]
fn is_aggregate_groupby_pipe_query() {
    assert!(is_aggregation_query(
        "source logs | groupby $l.subsystemname as region aggregate count() as total_logs"
    ));
}

#[test]
fn is_aggregate_count_pipe_query() {
    assert!(is_aggregation_query("source logs | count"));
}

#[test]
fn is_aggregate_distinct_pipe_query() {
    assert!(is_aggregation_query("source spans | distinct $d.traceID"));
}

#[test]
fn is_not_aggregate_filter_query() {
    assert!(!is_aggregation_query(
        r#"source logs | filter $d.severity == "ERROR""#
    ));
}

#[test]
fn is_not_aggregate_plain_source_query() {
    assert!(!is_aggregation_query("source logs"));
}

// ── Aggregate row normalisation ───────────────────────────────────────────────

#[test]
fn aggregate_row_fields_extracted_correctly() {
    let row =
        json!({"metadata": [], "labels": [], "userData": r#"{"region":"us1","total_logs":16}"#});
    let out = normalize_aggregate_row(&row);
    assert_eq!(out["region"], json!("us1"));
    assert_eq!(out["total_logs"], json!(16));
    assert!(
        out.get("metadata").is_none(),
        "envelope field 'metadata' must be absent"
    );
    assert!(
        out.get("labels").is_none(),
        "envelope field 'labels' must be absent"
    );
    assert!(
        out.get("userData").is_none(),
        "envelope field 'userData' must be absent"
    );
}

#[test]
fn aggregate_ndjson_full_pipeline() {
    // Simulate the full NDJSON payload from a groupby query (5 region rows)
    let rows = vec![
        json!({"metadata": [], "labels": [], "userData": r#"{"region":"us1","total_logs":16}"#}),
        json!({"metadata": [], "labels": [], "userData": r#"{"region":"us2","total_logs":20}"#}),
        json!({"metadata": [], "labels": [], "userData": r#"{"region":"us3","total_logs":12}"#}),
        json!({"metadata": [], "labels": [], "userData": r#"{"region":"ap2","total_logs":31}"#}),
        json!({"metadata": [], "labels": [], "userData": r#"{"region":"production","total_logs":2}"#}),
    ];

    let header = serde_json::to_string(
        &json!({"queryId": {"queryId": "85cdcaf7-de88-41d9-b1c4-a33f01ff2d36"}}),
    )
    .unwrap();
    let result = serde_json::to_string(&json!({"result": {"results": rows}})).unwrap();
    let ndjson = format!("{header}\n{result}\n");

    let (parsed_rows, warnings) = parse_ndjson_response(&ndjson).unwrap();
    assert_eq!(parsed_rows.len(), 5);
    assert!(warnings.is_empty());

    // Apply aggregate normalisation
    let results: Vec<Value> = parsed_rows.iter().map(normalize_aggregate_row).collect();
    assert_eq!(results.len(), 5);

    // Validate each row is a plain object with only the userData fields
    let regions: Vec<&str> = results
        .iter()
        .filter_map(|r| r["region"].as_str())
        .collect();
    assert!(regions.contains(&"us1"));
    assert!(regions.contains(&"us3"));
    assert!(regions.contains(&"production"));

    let totals: Vec<u64> = results
        .iter()
        .filter_map(|r| r["total_logs"].as_u64())
        .collect();
    assert_eq!(totals.iter().sum::<u64>(), 81);

    for r in &results {
        assert!(r.get("metadata").is_none());
        assert!(r.get("labels").is_none());
        assert!(r.get("userData").is_none());
    }
}

#[test]
fn aggregate_rows_do_not_affect_non_aggregate_log_parsing() {
    // Non-aggregate rows must still normalise through normalize_row unaffected
    let row = json!({
        "metadata": kv_array(&[("severity", "Error"), ("timestamp", "2026-03-21T09:00:00")]),
        "labels": kv_array(&[("applicationname", "api")]),
        "userData": r#"{"message":"test error"}"#
    });
    let out = normalize_row(&row);
    assert_eq!(out["metadata"]["severity"], json!("Error"));
    assert_eq!(out["labels"]["applicationname"], json!("api"));
    assert_eq!(out["userData"]["message"], json!("test error"));
    let record = parse_log_record(&out);
    assert_eq!(record.severity.as_deref(), Some("ERROR"));
}
