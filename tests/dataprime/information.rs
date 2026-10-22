//! Public CLI contract: validation, full stderr warnings, and extended stdout.
use serde_json::{json, Value};
use std::process::{Command, Output};
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn invoke(server: &MockServer, args: &[&str], profiles: &[&str]) -> Output {
    let home = std::env::temp_dir().join(format!("cx_query_info_{}", uuid::Uuid::new_v4()));
    let dir = home.join(".cx");
    std::fs::create_dir_all(dir.join("profiles")).unwrap();
    std::fs::write(
        dir.join("config.toml"),
        "default_profile = \"test\"\nmax_dataprime_direct_output_size = 1\n",
    )
    .unwrap();
    for profile in profiles {
        std::fs::write(dir.join("profiles").join(format!("{profile}.toml")), format!(
            "auth = \"api_key\"\ncredential_storage = \"file\"\napi_key = \"test-key\"\nregion = \"{}\"\n", server.uri()
        )).unwrap();
    }
    let mut command = Command::new(assert_cmd::cargo::cargo_bin!("cx"));
    command
        .env("CX_HOME", &home)
        .env_remove("CX_API_KEY")
        .env_remove("CX_REGION")
        .env_remove("CX_PROFILE")
        .env_remove("CX_OUTPUT")
        .env_remove("CX_AGENT_MODE")
        .env_remove("CLAUDECODE")
        .env_remove("CLAUDE_CODE")
        .args(["--no-console-link"]);
    for profile in profiles {
        command.args(["--profile", profile]);
    }
    command.args(args);
    let result = tokio::task::spawn_blocking(move || command.output().unwrap())
        .await
        .unwrap();
    std::fs::remove_dir_all(home).unwrap();
    result
}

fn stderr_warnings(output: &Output) -> Vec<Value> {
    String::from_utf8_lossy(&output.stderr)
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|v| v.get("warning").is_some())
        .collect()
}

#[tokio::test]
async fn strict_validation_defaults_true_and_can_be_disabled_on_all_query_commands() {
    for command in [vec!["logs"], vec!["spans"], vec!["dataprime", "query"]] {
        for flag in [
            None,
            Some("--strict-fields-validation"),
            Some("--strict-fields-validation=true"),
            Some("--strict-fields-validation=false"),
        ] {
            let server = MockServer::start().await;
            let strict = flag != Some("--strict-fields-validation=false");
            Mock::given(method("POST"))
                .and(path("/api/v1/dataprime/query"))
                .and(body_partial_json(
                    json!({"metadata": {"strictFieldsValidation": strict}}),
                ))
                .respond_with(
                    ResponseTemplate::new(200)
                        .set_body_string("{\"statistics\":{\"status\":\"COMPLETED\"}}"),
                )
                .expect(1)
                .mount(&server)
                .await;
            let mut args = command.clone();
            args.extend(["source logs | limit 0", "-o", "json"]);
            if let Some(flag) = flag {
                args.push(flag);
            }
            let output = invoke(&server, &args, &["test"]).await;
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(
                serde_json::from_slice::<Value>(&output.stdout).unwrap(),
                json!([])
            );
        }
    }
}

#[tokio::test]
async fn extended_information_preserves_full_warnings_and_statistics_in_all_formats() {
    let warning = json!({"archiveWarning": {"missingData": {}}});
    let statistics = json!({"status": "COMPLETED", "e2eDurationMs": "125", "outputRowCount": "1", "futureField": {"a": 1}});
    let ndjson = [
        json!({"queryId": {"queryId": "query-123"}}),
        json!({"warning": warning}),
        json!({"result": {"results": [{"metadata": [], "labels": [], "userData": "{\"message\":\"hello\"}"}]}}),
        json!({"statistics": statistics}),
    ].iter().map(Value::to_string).collect::<Vec<_>>().join("\n");
    for format in ["json", "text", "toon"] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/v1/dataprime/query"))
            .respond_with(ResponseTemplate::new(200).set_body_string(&ndjson))
            .mount(&server)
            .await;
        let output = invoke(
            &server,
            &["logs", "limit 1", "--extended-information", "-o", format],
            &["test"],
        )
        .await;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            stderr_warnings(&output),
            vec![json!({"profile": "test", "warning": warning})]
        );
        let stdout = String::from_utf8(output.stdout).unwrap();
        if format == "toon" {
            assert!(
                stdout.contains("query-123")
                    && stdout.contains("missingData")
                    && stdout.contains("statistics"),
                "{stdout}"
            );
        } else {
            let value: Value = if format == "text" {
                let (rows, information) = stdout.split_once("\n\n").unwrap();
                assert_eq!(rows, "- [INFO] hello");
                let information: Value = serde_json::from_str(information).unwrap();
                assert!(information.get("results").is_none());
                information
            } else {
                let value: Value = serde_json::from_str(&stdout).unwrap();
                assert_eq!(value["results"][0]["userData"]["message"], "hello");
                value
            };
            assert_eq!(value["queryId"], "query-123");
            assert_eq!(value["warnings"], json!([warning]));
            assert_eq!(value["statistics"], statistics);
        }
        assert!(
            !stdout.contains("Results written to:"),
            "extended information must remain on stdout"
        );
    }
}

#[tokio::test]
async fn warnings_without_message_are_emitted_even_on_failure() {
    for terminal in [
        json!({"statistics": {"status": "COMPLETED"}}),
        json!({"error": {"message": "engine failed"}}),
    ] {
        let server = MockServer::start().await;
        let warnings = [
            json!({"numberOfResultsLimitWarning": {"numberOfResultsLimit": 100}}),
            json!({"bytesScannedLimitWarning": {}}),
        ];
        let body = format!(
            "{}\n{}\n{}",
            json!({"warning": warnings[0]}),
            json!({"warning": warnings[1]}),
            terminal
        );
        Mock::given(method("POST"))
            .and(path("/api/v1/dataprime/query"))
            .respond_with(ResponseTemplate::new(200).set_body_string(body))
            .mount(&server)
            .await;
        let output = invoke(&server, &["logs", "limit 0", "-o", "json"], &["test"]).await;
        assert_eq!(output.status.success(), terminal.get("error").is_none());
        let emitted = stderr_warnings(&output);
        assert_eq!(emitted.len(), 2);
        assert_eq!(emitted[0]["warning"], warnings[0]);
        assert_eq!(emitted[1]["warning"], warnings[1]);
    }
}

#[tokio::test]
async fn extended_information_keeps_profile_queries_separate_and_handles_absent_metadata() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/dataprime/query"))
        .respond_with(ResponseTemplate::new(200).set_body_string("{\"result\":{\"results\":[]}}"))
        .mount(&server)
        .await;
    let output = invoke(
        &server,
        &[
            "dataprime",
            "query",
            "source logs | limit 0",
            "--extended-information",
            "-o",
            "json",
        ],
        &["prod", "staging"],
    )
    .await;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["results"], json!([]));
    assert_eq!(value["queries"].as_array().unwrap().len(), 2);
    for (query, profile) in value["queries"]
        .as_array()
        .unwrap()
        .iter()
        .zip(["prod", "staging"])
    {
        assert_eq!(query["profile"], profile);
        assert!(query["queryId"].is_null());
        assert!(query["statistics"].is_null());
        assert_eq!(query["warnings"], json!([]));
    }
}

#[tokio::test]
async fn raw_query_with_source_in_query_omits_unset_default_source() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/dataprime/query"))
        .and(|request: &wiremock::Request| {
            let body: Value = request.body_json().unwrap();
            body["metadata"].get("defaultSource").is_none()
                && body["query"] == "source logs | limit 0"
        })
        .respond_with(ResponseTemplate::new(200).set_body_string(
            "{\"queryId\":{\"queryId\":\"raw-query\"}}\n{\"statistics\":{\"status\":\"COMPLETED\"}}",
        ))
        .expect(1)
        .mount(&server)
        .await;
    let output = invoke(
        &server,
        &[
            "dataprime",
            "query",
            "source logs | limit 0",
            "--extended-information",
            "-o",
            "json",
        ],
        &["test"],
    )
    .await;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["queryId"], "raw-query");
    assert_eq!(value["results"], json!([]));
}

#[tokio::test]
async fn extended_text_preserves_result_rendering_across_commands_and_profiles() {
    let warning = json!({"archiveWarning": {"missingData": {}}});
    let statistics = json!({"status": "COMPLETED", "outputRowCount": "1", "futureField": {"a": 1}});
    let cases = [
        (
            "limit 1",
            json!({"message": "hello", "traceID": "trace-1", "spanID": "span-1"}),
        ),
        ("choose app", json!({"app": "api"})),
        ("aggregate count() as total", json!({"total": 3})),
    ];
    for command in [vec!["logs"], vec!["spans"], vec!["dataprime", "query"]] {
        for (query, data) in &cases {
            let ndjson = [
                json!({"queryId": {"queryId": "query-123"}}),
                json!({"warning": warning}),
                json!({"result": {"results": [{
                    "metadata": [
                        {"key": "timestamp", "value": "2026-01-01T00:00:00Z"},
                        {"key": "severity", "value": "5"},
                        {"key": "duration", "value": "5000"}
                    ],
                    "labels": [
                        {"key": "serviceName", "value": "gateway"},
                        {"key": "operationName", "value": "GET /health"}
                    ],
                    "userData": data.to_string()
                }]}}),
                json!({"statistics": statistics}),
            ]
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n");
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path("/api/v1/dataprime/query"))
                .respond_with(ResponseTemplate::new(200).set_body_string(&ndjson))
                .mount(&server)
                .await;
            for profiles in [vec!["test"], vec!["prod", "staging"]] {
                let mut args = command.clone();
                args.extend([query, "-o", "text"]);
                let plain = invoke(&server, &args, &profiles).await;
                args.push("--extended-information");
                let extended = invoke(&server, &args, &profiles).await;
                assert!(
                    plain.status.success(),
                    "{}",
                    String::from_utf8_lossy(&plain.stderr)
                );
                assert!(
                    extended.status.success(),
                    "{}",
                    String::from_utf8_lossy(&extended.stderr)
                );
                let plain_rows = String::from_utf8(plain.stdout.clone()).unwrap();
                let extended_stdout = String::from_utf8(extended.stdout.clone()).unwrap();
                let prefix = format!("{plain_rows}\n");
                let information: Value = serde_json::from_str(
                    extended_stdout
                        .strip_prefix(&prefix)
                        .expect("text result rendering must be unchanged"),
                )
                .unwrap();
                assert!(information.get("results").is_none());
                if profiles.len() == 1 {
                    assert_eq!(
                        information,
                        json!({
                            "queryId": "query-123", "warnings": [warning], "statistics": statistics
                        })
                    );
                } else {
                    assert_eq!(
                        information["queries"].as_array().unwrap().len(),
                        profiles.len()
                    );
                    for (info, profile) in information["queries"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .zip(&profiles)
                    {
                        assert_eq!(info["profile"], *profile);
                        assert_eq!(info["queryId"], "query-123");
                        assert_eq!(info["warnings"], json!([warning]));
                        assert_eq!(info["statistics"], statistics);
                    }
                }
                for output in [&plain, &extended] {
                    let emitted = stderr_warnings(output);
                    assert_eq!(emitted.len(), profiles.len());
                    for profile in &profiles {
                        assert!(emitted
                            .iter()
                            .any(|w| w == &json!({"profile": profile, "warning": warning})));
                    }
                }
            }
        }
    }
}
