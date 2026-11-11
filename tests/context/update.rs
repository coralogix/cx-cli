use serde_json::{json, Value};
use wiremock::matchers::{body_json, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::fixture::{cx, request_count, run, stderr, stdout, Home};

const ID: &str = "5b1c0e2a-0000-4000-8000-000000000002";
const KINDS: [(&str, &str); 2] = [("memories", "memory"), ("instructions", "instruction")];

fn entry(kind: &str, subjects: Value, deleted: bool, revision: u64) -> Value {
    let team_wide = subjects.as_array().unwrap().is_empty();
    json!({
        "id": ID, "kind": kind, "title": "Pool size", "summary": "Check it first.",
        "body": "Old body.\n", "subjects": subjects, "team_wide": team_wide,
        "source": "case 42", "change_note": "old note", "deleted": deleted, "revision": revision,
        "created_at": "2026-11-01T09:00:00Z", "updated_at": "2026-11-08T10:00:00Z",
        "updated_by": { "user_id": "u-1", "name": "Dana", "credential": "user_token" },
    })
}

fn receipt(entry: Value) -> Value {
    let subjects = entry["subjects"].clone();
    let team_wide = entry["team_wide"].clone();
    let in_full = entry["kind"] == "instruction";
    json!({
        "entry": entry,
        "found_by": { "context_for": subjects, "team_wide": team_wide, "returned_in_full": in_full },
    })
}

fn checkout() -> Value {
    json!([{ "type": "service", "value": "checkout" }])
}

fn entry_path(noun: &str) -> String {
    format!("/mgmt/api/v1/agent-context/{noun}/{ID}")
}

async fn serve_get(server: &MockServer, noun: &str, current: Value) {
    Mock::given(method("GET"))
        .and(path(entry_path(noun)))
        .respond_with(ResponseTemplate::new(200).set_body_json(current))
        .expect(1)
        .mount(server)
        .await;
}

async fn serve_put(
    server: &MockServer,
    noun: &str,
    request: Option<Value>,
    response: ResponseTemplate,
) {
    let mut mock = Mock::given(method("PUT")).and(path(entry_path(noun)));
    if let Some(request) = request {
        mock = mock.and(body_json(request));
    }
    mock.respond_with(response).expect(1).mount(server).await;
}

fn update_args<'a>(noun: &'a str, expected: &'a str, rest: &[&'a str]) -> Vec<&'a str> {
    let mut args = vec![
        "--yes",
        "context",
        noun,
        "update",
        ID,
        "--expected-revision",
        expected,
    ];
    args.extend_from_slice(rest);
    args
}

fn found_by(kind: &str, subjects: &str) -> String {
    match (kind, subjects) {
        ("memory", "") => "Found by: search, list --team-wide, and cx context get without subjects; a read for subjects only counts it.".into(),
        ("memory", s) => format!("Found by: cx context get {s} (and by search)"),
        (_, "") => "Returned in full by every cx context get.".into(),
        (_, s) => format!("Returned in full by: cx context get {s}"),
    }
}

#[tokio::test]
async fn update_reads_then_puts_the_merged_entry() {
    for (noun, kind) in KINDS {
        let server = MockServer::start().await;
        serve_get(&server, noun, entry(kind, checkout(), false, 4)).await;
        let request = json!({
            "title": "New title", "summary": "Check it first.", "body": "Old body.\n",
            "subjects": checkout(), "source": "case 42",
            "expected_revision": 4, "change_note": "renamed",
        });
        let mut updated = entry(kind, checkout(), false, 5);
        updated["title"] = json!("New title");
        let response = ResponseTemplate::new(200).set_body_json(receipt(updated));
        serve_put(&server, noun, Some(request), response).await;
        let home = Home::new();
        let args = update_args(
            noun,
            "4",
            &["--title", "New title", "--change-note", "renamed"],
        );
        let output = run(&home, &server, "text", &args);
        assert_eq!(output.status.code(), Some(0), "{output:?}");
        assert_eq!(
            stdout(&output),
            format!(
                "Updated {kind} {ID}, now at revision 5.\nSubjects: service:checkout\n{}\n",
                found_by(kind, "service:checkout")
            )
        );
        let methods: Vec<String> = server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .map(|r| r.method.to_string())
            .collect();
        assert_eq!(methods, ["GET", "PUT"]);
        let err = stderr(&output);
        assert!(
            err.contains(&format!("[auto-approved via --yes] Update {kind} {ID}?")),
            "{err}"
        );
        assert!(err.contains(&format!("Reading {kind}...")), "{err}");
        assert!(err.contains(&format!("Saving {kind}...")), "{err}");
    }
}

#[tokio::test]
async fn update_scope_transitions_and_source() {
    let alert = json!([{ "type": "alert", "value": "8f3a" }]);
    let cases: [(Value, &[&str], Value); 3] = [
        (
            checkout(),
            &["--team-wide"],
            json!({ "title": "Pool size", "summary": "Check it first.", "body": "Old body.\n",
                    "team_wide": true, "source": "case 42", "expected_revision": 2 }),
        ),
        (
            json!([]),
            &["--subject", "alert:8f3a"],
            json!({ "title": "Pool size", "summary": "Check it first.", "body": "Old body.\n",
                    "subjects": alert, "source": "case 42", "expected_revision": 2 }),
        ),
        (
            checkout(),
            &["--no-source"],
            json!({ "title": "Pool size", "summary": "Check it first.", "body": "Old body.\n",
                    "subjects": checkout(), "expected_revision": 2 }),
        ),
    ];
    for (subjects, flags, request) in cases {
        let server = MockServer::start().await;
        serve_get(&server, "memories", entry("memory", subjects, false, 2)).await;
        let sent_subjects = request.get("subjects").cloned().unwrap_or(json!([]));
        let response = ResponseTemplate::new(200).set_body_json(receipt(entry(
            "memory",
            sent_subjects,
            false,
            3,
        )));
        serve_put(&server, "memories", Some(request), response).await;
        let home = Home::new();
        let output = run(&home, &server, "text", &update_args("memories", "2", flags));
        assert_eq!(output.status.code(), Some(0), "{flags:?} {output:?}");
    }
}

#[tokio::test]
async fn update_body_from_file_and_stdin() {
    for from_stdin in [false, true] {
        let server = MockServer::start().await;
        serve_get(
            &server,
            "instructions",
            entry("instruction", checkout(), false, 1),
        )
        .await;
        let request = json!({
            "title": "Pool size", "summary": "Check it first.", "body": "New body.\n",
            "subjects": checkout(), "source": "case 42", "expected_revision": 1,
        });
        let response = ResponseTemplate::new(200).set_body_json(receipt(entry(
            "instruction",
            checkout(),
            false,
            2,
        )));
        serve_put(&server, "instructions", Some(request), response).await;
        let home = Home::new();
        let file = home.path().join("body.md");
        std::fs::write(&file, "New body.\n").unwrap();
        let file = file.to_string_lossy().into_owned();
        let source = if from_stdin { "-" } else { file.as_str() };
        let output = cx(&home, false)
            .args(["--api-key", "test", "--region", &server.uri(), "-o", "text"])
            .args(update_args("instructions", "1", &["--body-file", source]))
            .write_stdin("New body.\n")
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(0), "{output:?}");
    }
}

#[tokio::test]
async fn update_with_missing_body_file_sends_nothing() {
    let server = MockServer::start().await;
    let home = Home::new();
    let output = run(
        &home,
        &server,
        "text",
        &update_args("memories", "1", &["--body-file", "/nonexistent/body.md"]),
    );
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(stderr(&output).contains("Failed to read body file /nonexistent/body.md"));
    assert_eq!(request_count(&server).await, 0);
}

#[tokio::test]
async fn update_stops_before_writing_over_a_deleted_or_changed_entry() {
    for (noun, kind) in KINDS {
        let cases = [
            (
                entry(kind, checkout(), true, 4),
                format!("The {kind} is deleted. See cx context {noun} history {ID}, then restore it with revert."),
            ),
            (
                entry(kind, checkout(), false, 6),
                format!("The {kind} changed: it is at revision 6, not 4. Read it again (cx context {noun} get {ID}) and retry with --expected-revision 6."),
            ),
        ];
        for (current, message) in cases {
            let server = MockServer::start().await;
            serve_get(&server, noun, current).await;
            let home = Home::new();
            let output = run(
                &home,
                &server,
                "text",
                &update_args(noun, "4", &["--title", "X"]),
            );
            assert_eq!(output.status.code(), Some(1), "{output:?}");
            assert_eq!(stdout(&output), "");
            assert!(stderr(&output).contains(&message), "{output:?}");
            assert_eq!(request_count(&server).await, 1);
        }
    }
}

#[tokio::test]
async fn update_conflict_after_the_read_is_not_retried() {
    for (noun, kind) in KINDS {
        let server = MockServer::start().await;
        serve_get(&server, noun, entry(kind, checkout(), false, 4)).await;
        let error =
            json!({ "code": "revision_conflict", "message": "stale", "current_revision": 5 });
        serve_put(
            &server,
            noun,
            None,
            ResponseTemplate::new(409).set_body_json(error),
        )
        .await;
        let home = Home::new();
        let output = run(
            &home,
            &server,
            "text",
            &update_args(noun, "4", &["--title", "X"]),
        );
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert!(
            stderr(&output).contains(&format!(
                "The {kind} changed: it is at revision 5, not 4. Read it again (cx context {noun} get {ID}) and retry with --expected-revision 5."
            )),
            "{output:?}"
        );
        assert_eq!(request_count(&server).await, 2);
    }
}

#[tokio::test]
async fn update_of_missing_entry() {
    for (noun, kind) in KINDS {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(entry_path(noun)))
            .respond_with(
                ResponseTemplate::new(404)
                    .set_body_json(json!({ "code": "not_found", "message": "no entry" })),
            )
            .mount(&server)
            .await;
        let home = Home::new();
        let output = run(
            &home,
            &server,
            "text",
            &update_args(noun, "1", &["--title", "X"]),
        );
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert!(
            stderr(&output).contains(&format!(
                "No such {kind} {ID} in this team (or no such revision)."
            )),
            "{output:?}"
        );
    }
}

#[tokio::test]
async fn delete_sends_expected_revision_and_names_the_restore() {
    for (noun, kind) in KINDS {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path(entry_path(noun)))
            .and(query_param("expected_revision", "3"))
            .respond_with(ResponseTemplate::new(200).set_body_json(entry(
                kind,
                checkout(),
                true,
                4,
            )))
            .expect(1)
            .mount(&server)
            .await;
        let home = Home::new();
        let args = [
            "--yes",
            "context",
            noun,
            "delete",
            ID,
            "--expected-revision",
            "3",
        ];
        let output = run(&home, &server, "text", &args);
        assert_eq!(output.status.code(), Some(0), "{output:?}");
        assert_eq!(
            stdout(&output),
            format!("Deleted {kind} {ID}, now at revision 4.\nRestore with: cx context {noun} revert {ID} --to-revision 3 --expected-revision 4\n")
        );
        let err = stderr(&output);
        assert!(err.contains(&format!("Delete {kind} {ID}?")), "{err}");
        assert!(err.contains(&format!("Deleting {kind}...")), "{err}");
    }
}

#[tokio::test]
async fn delete_errors() {
    for (noun, kind) in KINDS {
        let cases = [
            (
                json!({ "code": "deleted", "message": "already deleted" }),
                format!("The {kind} is deleted. See cx context {noun} history {ID}, then restore it with revert."),
            ),
            (
                json!({ "code": "revision_conflict", "message": "stale", "current_revision": 7 }),
                format!("The {kind} changed: it is at revision 7, not 3."),
            ),
        ];
        for (error, message) in cases {
            let server = MockServer::start().await;
            Mock::given(method("DELETE"))
                .and(path(entry_path(noun)))
                .respond_with(ResponseTemplate::new(409).set_body_json(error))
                .mount(&server)
                .await;
            let home = Home::new();
            let args = [
                "--yes",
                "context",
                noun,
                "delete",
                ID,
                "--expected-revision",
                "3",
            ];
            let output = run(&home, &server, "text", &args);
            assert_eq!(output.status.code(), Some(1), "{output:?}");
            assert_eq!(stdout(&output), "");
            assert!(stderr(&output).contains(&message), "{output:?}");
        }
    }
}

async fn serve_revert(server: &MockServer, noun: &str, response: ResponseTemplate) {
    Mock::given(method("POST"))
        .and(path(format!("{}/revert", entry_path(noun))))
        .and(body_json(
            json!({ "to_revision": 2, "expected_revision": 4 }),
        ))
        .respond_with(response)
        .expect(1)
        .mount(server)
        .await;
}

const REVERT: [&str; 4] = ["--to-revision", "2", "--expected-revision", "4"];

fn revert_args(noun: &str) -> Vec<&str> {
    let mut args = vec!["--yes", "context", noun, "revert", ID];
    args.extend(REVERT);
    args
}

#[tokio::test]
async fn revert_sends_both_revisions_and_prints_the_receipt() {
    for (noun, kind) in KINDS {
        let server = MockServer::start().await;
        let response =
            ResponseTemplate::new(200).set_body_json(receipt(entry(kind, json!([]), false, 5)));
        serve_revert(&server, noun, response).await;
        let home = Home::new();
        let output = run(&home, &server, "text", &revert_args(noun));
        assert_eq!(output.status.code(), Some(0), "{output:?}");
        assert_eq!(
            stdout(&output),
            format!(
                "Reverted {kind} {ID} to revision 2, now at revision 5.\nSubjects: none (team-wide)\n{}\n",
                found_by(kind, "")
            )
        );
        let err = stderr(&output);
        assert!(
            err.contains(&format!("Revert {kind} {ID} to revision 2?")),
            "{err}"
        );
        assert!(err.contains(&format!("Reverting {kind}...")), "{err}");
    }
}

#[tokio::test]
async fn revert_to_a_deleted_revision() {
    let server = MockServer::start().await;
    let response =
        ResponseTemplate::new(200).set_body_json(receipt(entry("memory", checkout(), true, 5)));
    serve_revert(&server, "memories", response).await;
    let home = Home::new();
    let output = run(&home, &server, "text", &revert_args("memories"));
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(
        stdout(&output),
        format!("Reverted memory {ID} to revision 2, now at revision 5.\nSubjects: service:checkout\nThe entry is deleted; it is not returned by reads.\n")
    );
}

#[tokio::test]
async fn revert_to_a_missing_revision() {
    for (noun, kind) in KINDS {
        let server = MockServer::start().await;
        let error = json!({ "code": "not_found", "message": "no revision 2" });
        serve_revert(
            &server,
            noun,
            ResponseTemplate::new(404).set_body_json(error),
        )
        .await;
        let home = Home::new();
        let output = run(&home, &server, "text", &revert_args(noun));
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert!(
            stderr(&output).contains(&format!(
                "No such {kind} {ID} in this team (or no such revision)."
            )),
            "{output:?}"
        );
    }
}

#[tokio::test]
async fn writes_in_json_and_toon_print_the_service_response() {
    for format in ["json", "toon"] {
        let deleted = entry("memory", checkout(), true, 4);
        let updated = receipt(entry("memory", checkout(), false, 5));
        let reverted = receipt(entry("memory", checkout(), false, 6));

        let server = MockServer::start().await;
        serve_get(&server, "memories", entry("memory", checkout(), false, 4)).await;
        serve_put(
            &server,
            "memories",
            None,
            ResponseTemplate::new(200).set_body_json(updated.clone()),
        )
        .await;
        Mock::given(method("DELETE"))
            .and(path(entry_path("memories")))
            .respond_with(ResponseTemplate::new(200).set_body_json(deleted.clone()))
            .mount(&server)
            .await;
        serve_revert(
            &server,
            "memories",
            ResponseTemplate::new(200).set_body_json(reverted.clone()),
        )
        .await;

        let home = Home::new();
        let runs = [
            (
                update_args("memories", "4", &["--title", "X"]),
                updated,
                "Saving memory...",
            ),
            (
                vec![
                    "--yes",
                    "context",
                    "memories",
                    "delete",
                    ID,
                    "--expected-revision",
                    "3",
                ],
                deleted,
                "Deleting memory...",
            ),
            (revert_args("memories"), reverted, "Reverting memory..."),
        ];
        for (args, expected, status) in runs {
            let output = run(&home, &server, format, &args);
            assert_eq!(output.status.code(), Some(0), "{output:?}");
            let out = stdout(&output);
            let decoded: Value = if format == "json" {
                serde_json::from_str(&out).unwrap()
            } else {
                toon_format::decode_default(out.trim_end()).unwrap()
            };
            assert_eq!(decoded, expected);
            for text_only in [
                "Updated",
                "Deleted",
                "Reverted",
                "Restore with",
                "auto-approved",
            ] {
                assert!(!out.contains(text_only), "{out}");
            }
            let err = stderr(&output);
            assert!(
                err.contains(status) && err.contains("[auto-approved via --yes]"),
                "{err}"
            );
        }
    }
}

#[tokio::test]
async fn writes_without_yes_are_refused_before_any_request() {
    let writes: [&[&str]; 3] = [
        &[
            "update",
            ID,
            "--expected-revision",
            "1",
            "--body-file",
            "/nonexistent/body.md",
        ],
        &["delete", ID, "--expected-revision", "1"],
        &[
            "revert",
            ID,
            "--to-revision",
            "1",
            "--expected-revision",
            "2",
        ],
    ];
    for write in writes {
        for (agent, expected) in [
            (true, "You are running in agent mode"),
            (false, "stdin is not a terminal"),
        ] {
            let server = MockServer::start().await;
            let home = Home::new();
            let mut cmd = cx(&home, false);
            if agent {
                cmd.env("CX_AGENT_MODE", "1");
            }
            let output = cmd
                .args(["--api-key", "test", "--region", &server.uri(), "-o", "text"])
                .args(["context", "instructions"])
                .args(write)
                .output()
                .unwrap();
            assert_eq!(output.status.code(), Some(1), "{write:?} {output:?}");
            let err = stderr(&output);
            assert!(err.contains(expected), "{err}");
            assert!(!err.contains("body.md"), "{err}");
            assert_eq!(request_count(&server).await, 0);
        }
    }
}
