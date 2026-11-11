use serde_json::{json, Value};
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::common::test_target;
use crate::fixture::{request_count, run, stderr, stdout, Home};
use coralogix_cli::commands::context::run_get;
use coralogix_cli::config::OutputFormat;

const CONTEXT_PATH: &str = "/mgmt/api/v1/agent-context/context";
const ID1: &str = "7c1e0b1e-0000-4000-8000-000000000001";
const ID2: &str = "7c1e0b1e-0000-4000-8000-000000000002";
const MEM1: &str = "9a2f6c1e-0000-4000-8000-000000000003";
const MEM2: &str = "9a2f6c1e-0000-4000-8000-000000000004";

const GUIDANCE: [&str; 3] = [
    "This lookup matches subjects exactly. Read again with subjects you discover along the way,",
    "or search by symptom, error or component words: cx context memories search <PATTERN>",
    "No match does not mean nothing relevant exists.",
];
const NO_SUBJECTS: &str =
    "No subjects given: showing team-wide instructions and the most recent memories.";

fn author(name: Option<&str>) -> Value {
    json!({ "user_id": name.map(|_| "u-1"), "name": name,
            "credential": if name.is_some() { "user_token" } else { "team_api_key" } })
}

fn index(id: &str, kind: &str, title: &str, summary: &str, subjects: Value, who: Value) -> Value {
    json!({
        "id": id, "kind": kind, "title": title, "summary": summary, "subjects": subjects,
        "deleted": false, "revision": 3, "created_at": "2026-11-01T09:00:00Z",
        "updated_at": "2026-11-08T10:11:12.345Z", "updated_by": who,
    })
}

fn instruction(id: &str, level: &str, title: &str, subjects: Value, body: &str) -> Value {
    let mut entry = index(
        id,
        "instruction",
        title,
        "s",
        subjects.clone(),
        author(Some("Dana")),
    );
    entry["body"] = json!(body);
    entry["source"] = Value::Null;
    entry["team_wide"] = json!(level == "team");
    entry["change_note"] = Value::Null;
    let matched = if level == "team" { json!([]) } else { subjects };
    json!({ "level": level, "matched": matched, "entry": entry })
}

fn memory(id: &str, level: &str, title: &str, matched: Value) -> Value {
    let mut m = index(
        id,
        "memory",
        title,
        "Retry storms follow each deploy.",
        matched.clone(),
        author(None),
    );
    m["level"] = json!(level);
    m["matched"] = matched;
    m
}

fn mixed() -> Value {
    json!({
        "instructions": [
            instruction(ID1, "team", "Paging policy", json!([]), "Page the on-call.\n"),
            instruction(ID2, "subject", "Check the pool", json!([{ "type": "alert", "value": "8f3a" }]), "Look at the pool size."),
        ],
        "memories": {
            "items": [
                memory(MEM1, "subject", "Checkout timeouts", json!([{ "type": "service", "value": "checkout" }])),
                memory(MEM2, "subject", "Odd host", json!([{ "type": "host", "value": "web 01" }])),
            ],
            "total": 14, "limit": 2, "offset": 3,
        },
        "team_wide_memories": 6,
        "memories_total": 40,
    })
}

fn empty() -> Value {
    json!({
        "instructions": [],
        "memories": { "items": [], "total": 0, "limit": 50, "offset": 0 },
        "team_wide_memories": 0,
        "memories_total": 0,
    })
}

async fn serve(response: Value, expected_body: Value) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(CONTEXT_PATH))
        .and(body_json(expected_body))
        .respond_with(ResponseTemplate::new(200).set_body_json(response))
        .expect(1)
        .mount(&server)
        .await;
    server
}

fn ok_stdout(output: &std::process::Output) -> String {
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(stderr(output).contains("Reading context..."), "{output:?}");
    stdout(output)
}

const MIXED_ARGS: [&str; 8] = [
    "context",
    "get",
    "service:checkout",
    "host:web 01",
    "--limit",
    "2",
    "--offset",
    "3",
];

fn mixed_request() -> Value {
    json!({
        "subjects": [{ "type": "service", "value": "checkout" }, { "type": "host", "value": "web 01" }],
        "limit": 2, "offset": 3,
    })
}

fn mixed_hints() -> Vec<String> {
    let mut hints = vec!["Not shown: 9 more memories for these subjects (--offset 5); 6 team-wide memories (cx context memories list --team-wide).".to_string()];
    hints.extend(GUIDANCE.map(String::from));
    hints
}

#[tokio::test]
async fn text_for_mixed_response_with_offset() {
    let server = serve(mixed(), mixed_request()).await;
    let home = Home::new();
    let out = ok_stdout(&run(&home, &server, "text", &MIXED_ARGS));
    let expected = [
        "Instructions (2)",
        "",
        "[team-wide] Paging policy",
        &format!("instruction {ID1}  rev 3  updated 2026-11-08 by Dana"),
        "Page the on-call.",
        "",
        "[alert:8f3a] Check the pool",
        &format!("instruction {ID2}  rev 3  updated 2026-11-08 by Dana"),
        "Look at the pool size.",
        "",
        "Memories (4-5 of 14)",
        "",
        &format!("{MEM1}  2026-11-08  [service:checkout]  Checkout timeouts"),
        "    Retry storms follow each deploy.",
        &format!("{MEM2}  2026-11-08  ['host:web 01']  Odd host"),
        "    Retry storms follow each deploy.",
        "",
    ]
    .map(String::from)
    .into_iter()
    .chain(mixed_hints())
    .collect::<Vec<_>>()
    .join("\n");
    assert_eq!(out, format!("{expected}\n"));
}

#[tokio::test]
async fn json_is_response_plus_hints() {
    let server = serve(mixed(), mixed_request()).await;
    let home = Home::new();
    let out = ok_stdout(&run(&home, &server, "json", &MIXED_ARGS));
    let mut expected = mixed();
    expected["hints"] = json!(mixed_hints());
    assert_eq!(serde_json::from_str::<Value>(&out).unwrap(), expected);
}

#[tokio::test]
async fn toon_is_response_plus_hints() {
    let server = serve(mixed(), mixed_request()).await;
    let home = Home::new();
    let out = ok_stdout(&run(&home, &server, "toon", &MIXED_ARGS));
    let mut expected = mixed();
    expected["hints"] = json!(mixed_hints());
    let decoded: Value = toon_format::decode_default(out.trim_end()).unwrap();
    assert_eq!(decoded, expected);
}

#[tokio::test]
async fn empty_response_in_three_formats() {
    let request = json!({ "subjects": [], "limit": 50, "offset": 0 });
    let hints: Vec<String> = [NO_SUBJECTS, GUIDANCE[1], GUIDANCE[2]]
        .map(String::from)
        .to_vec();
    let mut expected = empty();
    expected["hints"] = json!(hints);

    let server = serve(empty(), request.clone()).await;
    let home = Home::new();
    let text = ok_stdout(&run(&home, &server, "text", &["context", "get"]));
    assert_eq!(
        text,
        format!("No instructions or memories yet.\n\n{}\n", hints.join("\n"))
    );

    let server = serve(empty(), request.clone()).await;
    let json_out = ok_stdout(&run(&home, &server, "json", &["context", "get"]));
    assert_eq!(serde_json::from_str::<Value>(&json_out).unwrap(), expected);

    let server = serve(empty(), request).await;
    let toon = ok_stdout(&run(&home, &server, "toon", &["context", "get"]));
    let decoded: Value = toon_format::decode_default(toon.trim_end()).unwrap();
    assert_eq!(decoded, expected);
}

#[tokio::test]
async fn offset_past_the_end_without_subjects() {
    let response = json!({
        "instructions": [],
        "memories": { "items": [], "total": 14, "limit": 50, "offset": 20 },
        "team_wide_memories": 2,
        "memories_total": 14,
    });
    let server = serve(
        response,
        json!({ "subjects": [], "limit": 50, "offset": 20 }),
    )
    .await;
    let home = Home::new();
    let out = ok_stdout(&run(
        &home,
        &server,
        "text",
        &["context", "get", "--offset", "20"],
    ));
    assert_eq!(
        out,
        [
            "Memories (0 of 14)",
            "",
            "No memories at this offset; there are 14 (use --offset 0).",
            "",
            NO_SUBJECTS,
            GUIDANCE[1],
            GUIDANCE[2],
            "",
        ]
        .join("\n")
    );
}

#[tokio::test]
async fn recent_memories_without_subjects() {
    let response = json!({
        "instructions": [],
        "memories": { "items": [memory(MEM1, "recent", "Checkout timeouts", json!([]))],
                      "total": 3, "limit": 1, "offset": 0 },
        "team_wide_memories": 1,
        "memories_total": 3,
    });
    let server = serve(response, json!({ "subjects": [], "limit": 1, "offset": 0 })).await;
    let home = Home::new();
    let out = ok_stdout(&run(
        &home,
        &server,
        "text",
        &["context", "get", "--limit", "1"],
    ));
    assert!(out.starts_with(&format!(
        "Memories (1-1 of 3)\n\n{MEM1}  2026-11-08  [recent]  Checkout timeouts\n"
    )));
    assert!(out.contains("\nNot shown: 2 more recent memories (--offset 1).\n"));
}

#[tokio::test]
async fn limit_bounds_reach_the_server_unchanged() {
    for limit in [1, 200] {
        let server = serve(
            empty(),
            json!({ "subjects": [{ "type": "alert" }], "limit": limit, "offset": 0 }),
        )
        .await;
        let home = Home::new();
        let output = run(
            &home,
            &server,
            "json",
            &["context", "get", "alert", "--limit", &limit.to_string()],
        );
        ok_stdout(&output);
    }
}

async fn failing(status: u16, body: &str) -> std::process::Output {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(CONTEXT_PATH))
        .respond_with(ResponseTemplate::new(status).set_body_string(body.to_string()))
        .mount(&server)
        .await;
    let home = Home::new();
    run(&home, &server, "text", &["context", "get", "service:x"])
}

#[tokio::test]
async fn structured_not_found_prints_service_message() {
    let output = failing(404, r#"{"code":"not_found","message":"team not found"}"#).await;
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(stderr(&output).contains("team not found"), "{output:?}");
    assert!(stdout(&output).is_empty());
}

#[tokio::test]
async fn unstructured_404_is_not_available() {
    let output = failing(404, "404 page not found").await;
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(
        stderr(&output).contains("Agent context is not available in this environment."),
        "{output:?}"
    );
}

#[tokio::test]
async fn gateway_failure_is_generic() {
    let output = failing(502, "<html>Bad Gateway</html>").await;
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(
        stderr(&output).contains("API request failed (502): <html>Bad Gateway</html>"),
        "{output:?}"
    );
}

#[tokio::test]
async fn invalid_subject_is_refused_before_any_request() {
    let server = MockServer::start().await;
    let home = Home::new();
    let output = run(&home, &server, "text", &["context", "get", "alert:"]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(stderr(&output).contains("Invalid subject 'alert:'"));
    assert_eq!(request_count(&server).await, 0);
}

#[tokio::test]
async fn runner_refuses_two_targets() {
    let server = MockServer::start().await;
    let targets = [
        test_target("a", &server.uri()),
        test_target("b", &server.uri()),
    ];
    let err = run_get(&targets, &[], 50, 0, OutputFormat::Json)
        .await
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        "cx context does not support multi-profile. Use a single profile with -p <profile>."
    );
    assert_eq!(request_count(&server).await, 0);
}

#[tokio::test]
async fn malformed_success_body_fails_without_output() {
    for format in ["text", "json"] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(CONTEXT_PATH))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .mount(&server)
            .await;
        let home = Home::new();
        let output = run(&home, &server, format, &["context", "get"]);
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert_eq!(stdout(&output), "");
        assert!(stderr(&output).contains("missing field"), "{output:?}");
    }
}
