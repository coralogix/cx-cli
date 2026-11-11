use std::time::Duration;

use serde_json::{json, Value};
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::fixture::{cx, request_count, run, stderr, stdout, Home};

const ID: &str = "5b1c0e2a-0000-4000-8000-000000000001";
const KINDS: [(&str, &str); 2] = [("memories", "memory"), ("instructions", "instruction")];
const RETRY: &str =
    "To retry without saving twice, pass --id 5b1c0e2a-0000-4000-8000-000000000001.";

fn receipt(kind: &str, subjects: Value, deleted: bool, revision: u64) -> Value {
    let team_wide = subjects.as_array().unwrap().is_empty();
    json!({
        "entry": {
            "id": ID, "kind": kind, "title": "Pool size", "summary": "Check it first.",
            "body": "The body.\n", "subjects": subjects, "team_wide": team_wide,
            "source": null, "change_note": null, "deleted": deleted, "revision": revision,
            "created_at": "2026-11-01T09:00:00Z", "updated_at": "2026-11-08T10:00:00Z",
            "updated_by": { "user_id": null, "name": null, "credential": "team_api_key" },
        },
        "found_by": { "context_for": subjects, "team_wide": team_wide, "returned_in_full": kind == "instruction" },
    })
}

fn checkout() -> Value {
    json!([{ "type": "service", "value": "checkout" }, { "type": "ip", "value": "2001:db8::" }])
}

async fn serve(noun: &str, request: Option<Value>, response: ResponseTemplate) -> MockServer {
    let server = MockServer::start().await;
    let mut mock =
        Mock::given(method("POST")).and(path(format!("/mgmt/api/v1/agent-context/{noun}")));
    if let Some(request) = request {
        mock = mock.and(body_json(request));
    }
    mock.respond_with(response).expect(1).mount(&server).await;
    server
}

fn body_file(home: &Home) -> String {
    let file = home.path().join("body.md");
    std::fs::write(&file, "The body.\n").unwrap();
    file.to_string_lossy().into_owned()
}

fn create_args<'a>(noun: &'a str, body: &'a str, scope: &[&'a str]) -> Vec<&'a str> {
    let mut args = vec![
        "--yes",
        "context",
        noun,
        "create",
        "--title",
        "Pool size",
        "--summary",
        "Check it first.",
        "--body-file",
        body,
        "--id",
        ID,
    ];
    args.extend_from_slice(scope);
    args
}

const SUBJECT_FLAGS: [&str; 4] = [
    "--subject",
    "service:checkout",
    "--subject",
    "ip:2001:db8::",
];

#[tokio::test]
async fn create_with_subjects_saves_and_prints_receipt() {
    for (noun, kind) in KINDS {
        let request = json!({
            "id": ID, "title": "Pool size", "summary": "Check it first.", "body": "The body.\n",
            "subjects": checkout(), "source": "case 42",
        });
        let response =
            ResponseTemplate::new(201).set_body_json(receipt(kind, checkout(), false, 1));
        let server = serve(noun, Some(request), response).await;
        let home = Home::new();
        let body = body_file(&home);
        let mut args = create_args(noun, &body, &SUBJECT_FLAGS);
        args.extend(["--source", "case 42"]);
        let output = run(&home, &server, "text", &args);
        assert_eq!(output.status.code(), Some(0), "{output:?}");
        let found_by = match kind {
            "memory" => "Found by: cx context get service:checkout ip:2001:db8:: (and by search)",
            _ => "Returned in full by: cx context get service:checkout ip:2001:db8::",
        };
        assert_eq!(
            stdout(&output),
            format!("Saved {kind} {ID}, revision 1.\nSubjects: service:checkout ip:2001:db8::\n{found_by}\n")
        );
        let err = stderr(&output);
        assert!(err.contains(&format!("Saving {kind}...")), "{err}");
        assert!(err.contains("[auto-approved via --yes] Create "), "{err}");
        assert!(err.contains("'Pool size'?"), "{err}");
    }
}

#[tokio::test]
async fn create_team_wide_already_saved() {
    for (noun, kind) in KINDS {
        let request = json!({
            "id": ID, "title": "Pool size", "summary": "Check it first.", "body": "The body.\n",
            "team_wide": true,
        });
        let response = ResponseTemplate::new(200).set_body_json(receipt(kind, json!([]), false, 4));
        let server = serve(noun, Some(request), response).await;
        let home = Home::new();
        let body = body_file(&home);
        let output = run(
            &home,
            &server,
            "text",
            &create_args(noun, &body, &["--team-wide"]),
        );
        assert_eq!(output.status.code(), Some(0), "{output:?}");
        let found_by = match kind {
            "memory" => "Found by: search, list --team-wide, and cx context get without subjects; a read for subjects only counts it.",
            _ => "Returned in full by every cx context get.",
        };
        assert_eq!(
            stdout(&output),
            format!("Already saved: {kind} {ID}, now at revision 4.\nSubjects: none (team-wide)\n{found_by}\n")
        );
    }
}

#[tokio::test]
async fn create_of_since_deleted_entry_says_so() {
    let response = ResponseTemplate::new(200).set_body_json(receipt("memory", checkout(), true, 3));
    let server = serve("memories", None, response).await;
    let home = Home::new();
    let body = body_file(&home);
    let output = run(
        &home,
        &server,
        "text",
        &create_args("memories", &body, &SUBJECT_FLAGS),
    );
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(
        stdout(&output),
        format!("Already saved: memory {ID}, now at revision 3.\nSubjects: service:checkout ip:2001:db8::\nThe entry is deleted; it is not returned by reads.\n")
    );
}

#[tokio::test]
async fn create_json_and_toon_print_the_receipt() {
    for format in ["json", "toon"] {
        let body_json_value = receipt("memory", checkout(), false, 1);
        let response = ResponseTemplate::new(201).set_body_json(body_json_value.clone());
        let server = serve("memories", None, response).await;
        let home = Home::new();
        let body = body_file(&home);
        let output = run(
            &home,
            &server,
            format,
            &create_args("memories", &body, &SUBJECT_FLAGS),
        );
        assert_eq!(output.status.code(), Some(0), "{output:?}");
        let out = stdout(&output);
        let decoded: Value = if format == "json" {
            serde_json::from_str(&out).unwrap()
        } else {
            toon_format::decode_default(out.trim_end()).unwrap()
        };
        assert_eq!(decoded, body_json_value);
        assert!(!out.contains("Saved"), "{out}");
        assert!(!out.contains("auto-approved"), "{out}");
        assert!(stderr(&output).contains("Saving memory..."), "{output:?}");
    }
}

#[tokio::test]
async fn create_generates_a_uuid_without_id() {
    let response =
        ResponseTemplate::new(201).set_body_json(receipt("memory", checkout(), false, 1));
    let server = serve("memories", None, response).await;
    let home = Home::new();
    let body = body_file(&home);
    let args = [
        "--yes",
        "context",
        "memories",
        "create",
        "--title",
        "T",
        "--summary",
        "S",
        "--body-file",
        &body,
        "--subject",
        "service:checkout",
    ];
    let output = run(&home, &server, "text", &args);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let requests = server.received_requests().await.unwrap();
    let sent: Value = serde_json::from_slice(&requests[0].body).unwrap();
    let id = uuid::Uuid::parse_str(sent["id"].as_str().unwrap()).unwrap();
    assert_eq!(id.get_version_num(), 4);
}

#[tokio::test]
async fn create_reads_the_body_from_stdin() {
    let request = json!({
        "id": ID, "title": "Pool size", "summary": "Check it first.", "body": "from stdin\n",
        "subjects": checkout(),
    });
    let response =
        ResponseTemplate::new(201).set_body_json(receipt("memory", checkout(), false, 1));
    let server = serve("memories", Some(request), response).await;
    let home = Home::new();
    let output = cx(&home, false)
        .args(["--api-key", "test", "--region", &server.uri(), "-o", "text"])
        .args(create_args("memories", "-", &SUBJECT_FLAGS))
        .write_stdin("from stdin\n")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0), "{output:?}");
}

#[tokio::test]
async fn create_id_conflict() {
    for (noun, kind) in KINDS {
        let error = json!({ "code": "id_conflict", "message": "id exists" });
        let server = serve(noun, None, ResponseTemplate::new(409).set_body_json(error)).await;
        let home = Home::new();
        let body = body_file(&home);
        let output = run(
            &home,
            &server,
            "text",
            &create_args(noun, &body, &["--team-wide"]),
        );
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert_eq!(stdout(&output), "");
        let err = stderr(&output);
        assert!(
            err.contains(&format!("A {kind} or another entry with this ID exists with different content. Omit --id to create a new entry.\n{RETRY}")),
            "{err}"
        );
    }
}

#[tokio::test]
async fn failed_create_names_the_id_to_retry_with() {
    let cases = [
        (
            ResponseTemplate::new(500).set_body_string("boom"),
            "API request failed (500)",
        ),
        (
            ResponseTemplate::new(401).set_body_string("nope"),
            "cx profiles add",
        ),
        (ResponseTemplate::new(429), "429"),
        (
            ResponseTemplate::new(201).set_delay(Duration::from_secs(3)),
            "HTTP request timed out",
        ),
    ];
    for (response, expected) in cases {
        let server = serve("memories", None, response).await;
        let home = Home::new();
        let body = body_file(&home);
        let mut args = vec!["--http-timeout", "1"];
        args.extend(create_args("memories", &body, &["--team-wide"]));
        let output = run(&home, &server, "text", &args);
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        let err = stderr(&output);
        assert!(err.contains(expected), "{err}");
        assert!(err.contains(RETRY), "{err}");
    }
}

#[tokio::test]
async fn create_without_yes_is_refused_before_reading_the_body() {
    let cases: [(bool, &str); 2] = [
        (true, "You are running in agent mode"),
        (false, "stdin is not a terminal"),
    ];
    for (agent, expected) in cases {
        let server = MockServer::start().await;
        let home = Home::new();
        let mut cmd = cx(&home, false);
        if agent {
            cmd.env("CX_AGENT_MODE", "1");
        }
        let output = cmd
            .args(["--api-key", "test", "--region", &server.uri(), "-o", "text"])
            .args([
                "context",
                "memories",
                "create",
                "--title",
                "T",
                "--summary",
                "S",
                "--body-file",
                "/nonexistent/body.md",
                "--team-wide",
            ])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        let err = stderr(&output);
        assert!(err.contains(expected), "{err}");
        assert_eq!(agent, err.contains("Create a memory 'T'?"), "{err}");
        assert!(!err.contains("body.md"), "{err}");
        assert_eq!(request_count(&server).await, 0);
    }
}

#[tokio::test]
async fn create_with_missing_body_file_sends_nothing() {
    let server = MockServer::start().await;
    let home = Home::new();
    let output = run(
        &home,
        &server,
        "text",
        &create_args("memories", "/nonexistent/body.md", &["--team-wide"]),
    );
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(stderr(&output).contains("Failed to read body file /nonexistent/body.md"));
    assert_eq!(request_count(&server).await, 0);
}
