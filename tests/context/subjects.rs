use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::fixture::{request_count, run, stderr, stdout, Home};

const TYPES_PATH: &str = "/mgmt/api/v1/agent-context/subject-types";

#[tokio::test]
async fn subjects_text_json_toon() {
    let response = json!({
        "types": [
            { "type": "host", "memories": 4, "instructions": 0 },
            { "type": "alert", "memories": 2, "instructions": 1 },
        ],
        "total": 2,
    });
    for format in ["text", "json", "toon"] {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(TYPES_PATH))
            .respond_with(ResponseTemplate::new(200).set_body_json(response.clone()))
            .expect(1)
            .mount(&server)
            .await;
        let home = Home::new();
        let output = run(&home, &server, format, &["context", "subjects"]);
        assert_eq!(output.status.code(), Some(0), "{output:?}");
        assert!(stderr(&output).contains("Reading subject types..."));
        let out = stdout(&output);
        match format {
            "text" => {
                let host = out.find("host").unwrap();
                let alert = out.find("alert").unwrap();
                let slo = out.find("slo").unwrap();
                assert!(host < alert && alert < slo, "{out}");
                assert!(out.contains("cx alerts list --name <text>"), "{out}");
                assert!(out.contains("reuse a type already in use"), "{out}");
            }
            "json" => {
                let value: serde_json::Value = serde_json::from_str(&out).unwrap();
                assert_eq!(value["total"], 2);
                assert_eq!(value["types"].as_array().unwrap().len(), 6);
                assert_eq!(value["types"][0]["type"], "host");
                assert_eq!(value["types"][0]["coralogix_id"], json!(null));
                assert_eq!(value["types"][2]["type"], "case");
                assert_eq!(value["types"][2]["memories"], 0);
            }
            _ => assert!(out.contains("types[6]"), "{out}"),
        }
    }
}

#[tokio::test]
async fn a_write_with_a_wrong_id_form_sends_nothing() {
    let server = MockServer::start().await;
    let home = Home::new();
    let cases: [&[&str]; 3] = [
        &[
            "context",
            "memories",
            "create",
            "--yes",
            "--title",
            "t",
            "--summary",
            "s",
            "--body-file",
            "/dev/null",
            "--subject",
            "alert:checkout-error-rate",
        ],
        &[
            "context",
            "instructions",
            "create",
            "--yes",
            "--title",
            "t",
            "--summary",
            "s",
            "--body-file",
            "/dev/null",
            "--subject",
            "dataset:logs",
        ],
        &[
            "context",
            "memories",
            "update",
            "9a2f6c1e-0000-4000-8000-000000000001",
            "--expected-revision",
            "1",
            "--yes",
            "--subject",
            "case:CASE-123",
        ],
    ];
    for args in cases {
        let output = run(&home, &server, "text", args);
        assert_eq!(output.status.code(), Some(1), "{args:?} {output:?}");
        let err = stderr(&output);
        assert!(err.contains("does not look like"), "{err}");
        assert!(err.contains("Find it with:"), "{err}");
    }
    assert_eq!(request_count(&server).await, 0);
}

#[tokio::test]
async fn a_read_with_a_wrong_id_form_warns_and_reads() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/mgmt/api/v1/agent-context/context"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "instructions": [], "memories_total": 0,
            "memories": { "items": [], "total": 0, "limit": 50, "offset": 0 },
            "team_wide_memories": 0,
        })))
        .expect(1)
        .mount(&server)
        .await;
    let home = Home::new();
    let output = run(&home, &server, "text", &["context", "get", "alert:by-name"]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let err = stderr(&output);
    assert!(err.contains("warning:"), "{err}");
    assert!(
        err.contains("'alert:by-name' does not look like an alert"),
        "{err}"
    );
}
