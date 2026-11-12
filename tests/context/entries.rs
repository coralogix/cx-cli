use serde_json::{json, Value};
use wiremock::matchers::{body_json, method, path, query_param, query_param_is_missing};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::fixture::{request_count, run, shell, stderr, stdout, Home};

const ID: &str = "9a2f6c1e-0000-4000-8000-000000000001";
const ID2: &str = "9a2f6c1e-0000-4000-8000-000000000002";
const KINDS: [(&str, &str); 2] = [("memories", "memory"), ("instructions", "instruction")];

fn query_path(noun: &str) -> String {
    format!("/mgmt/api/v1/agent-context/{noun}/query")
}

fn index(id: &str, kind: &str, subjects: Value, deleted: bool) -> Value {
    json!({
        "id": id, "kind": kind, "title": "Checkout timeouts", "summary": "Retry storms follow deploys.",
        "subjects": subjects, "deleted": deleted, "revision": 2,
        "created_at": "2026-11-01T09:00:00Z", "updated_at": "2026-11-08T10:11:12Z",
        "updated_by": { "user_id": null, "name": null, "credential": "team_api_key" },
    })
}

fn page(items: Vec<Value>, total: u64, offset: u64) -> Value {
    json!({ "items": items, "total": total, "limit": 50, "offset": offset })
}

fn two_items(kind: &str) -> Value {
    page(
        vec![
            index(
                ID,
                kind,
                json!([{ "type": "service", "value": "checkout" }]),
                false,
            ),
            index(ID2, kind, json!([]), true),
        ],
        7,
        2,
    )
}

async fn serve_query(noun: &str, request: Value, response: Value) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(query_path(noun)))
        .and(body_json(request))
        .respond_with(ResponseTemplate::new(200).set_body_json(response))
        .expect(1)
        .mount(&server)
        .await;
    server
}

fn ok(output: &std::process::Output, status: &str) -> String {
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(stderr(output).contains(status), "{output:?}");
    stdout(output)
}

#[tokio::test]
async fn list_request_bodies() {
    let cases: [(&[&str], Value); 3] = [
        (&[], json!({ "limit": 50, "offset": 0 })),
        (
            &[
                "--subject",
                "service:checkout",
                "--subject",
                "alert",
                "--include-deleted",
                "--limit",
                "5",
                "--offset",
                "2",
            ],
            json!({ "subjects": [{ "type": "service", "value": "checkout" }, { "type": "alert" }],
                    "include_deleted": true, "limit": 5, "offset": 2 }),
        ),
        (
            &["--team-wide"],
            json!({ "team_wide": true, "limit": 50, "offset": 0 }),
        ),
    ];
    for (noun, kind) in KINDS {
        for (flags, request) in &cases {
            let server = serve_query(noun, request.clone(), page(vec![], 0, 0)).await;
            let home = Home::new();
            let mut args = vec!["context", noun, "list"];
            args.extend(*flags);
            let out = ok(
                &run(&home, &server, "text", &args),
                &format!("Listing {noun}..."),
            );
            assert_eq!(out, format!("No {noun} found.\n"), "{kind}");
        }
    }
}

#[tokio::test]
async fn search_request_bodies() {
    let cases: [(&[&str], Value); 4] = [
        (
            &["timeout"],
            json!({ "patterns": ["timeout"], "limit": 50, "offset": 0 }),
        ),
        (
            &[
                "a.b",
                "-e",
                "timed out",
                "-e",
                "x|y",
                "-F",
                "--subject",
                "ip:2001:db8::",
                "--include-deleted",
                "--offset",
                "4",
            ],
            json!({ "patterns": ["a.b", "timed out", "x|y"], "fixed": true,
                    "subjects": [{ "type": "ip", "value": "2001:db8::" }],
                    "include_deleted": true, "limit": 50, "offset": 4 }),
        ),
        (
            &["pool", "--team-wide", "--limit", "200"],
            json!({ "patterns": ["pool"], "team_wide": true, "limit": 200, "offset": 0 }),
        ),
        (
            &["-e", "pool", "-e", "retry"],
            json!({ "patterns": ["pool", "retry"], "limit": 50, "offset": 0 }),
        ),
    ];
    for (noun, _) in KINDS {
        for (flags, request) in &cases {
            let server = serve_query(noun, request.clone(), page(vec![], 0, 0)).await;
            let home = Home::new();
            let mut args = vec!["context", noun, "search"];
            args.extend(*flags);
            ok(
                &run(&home, &server, "text", &args),
                &format!("Searching {noun}..."),
            );
        }
    }
}

#[tokio::test]
async fn list_text_json_toon() {
    for (noun, kind) in KINDS {
        let request = json!({ "include_deleted": true, "limit": 50, "offset": 2 });
        let args = [
            "context",
            noun,
            "list",
            "--include-deleted",
            "--offset",
            "2",
        ];
        let home = Home::new();

        let server = serve_query(noun, request.clone(), two_items(kind)).await;
        let text = ok(&run(&home, &server, "text", &args), "Listing");
        assert_eq!(
            text,
            format!(
                "{ID}  2026-11-08  [service:checkout]  Checkout timeouts\n    Retry storms follow deploys.\n\
                 {ID2}  2026-11-08  [team-wide]  Checkout timeouts (deleted)\n    Retry storms follow deploys.\n\
                 \nShowing 3-4 of 7. More: --offset 4\n"
            )
        );

        let server = serve_query(noun, request.clone(), two_items(kind)).await;
        let out = ok(&run(&home, &server, "json", &args), "Listing");
        assert_eq!(
            serde_json::from_str::<Value>(&out).unwrap(),
            two_items(kind)
        );

        let server = serve_query(noun, request, two_items(kind)).await;
        let out = ok(&run(&home, &server, "toon", &args), "Listing");
        let decoded: Value = toon_format::decode_default(out.trim_end()).unwrap();
        assert_eq!(decoded, two_items(kind));
    }
}

#[tokio::test]
async fn list_offset_past_the_end() {
    for (noun, _) in KINDS {
        let server = serve_query(
            noun,
            json!({ "limit": 50, "offset": 30 }),
            page(vec![], 7, 30),
        )
        .await;
        let home = Home::new();
        let out = ok(
            &run(
                &home,
                &server,
                "text",
                &["context", noun, "list", "--offset", "30"],
            ),
            "Listing",
        );
        assert_eq!(out, format!("No {noun} at this offset; there are 7.\n"));
    }
}

fn search_page(kind: &str) -> Value {
    let m = |field: &str, line: u64, text: &str, s: bool, e: bool| json!({ "field": field, "line": line, "text": text, "cut_start": s, "cut_end": e });
    let mut hit = index(
        ID,
        kind,
        json!([{ "type": "alert", "value": "8f3a" }]),
        false,
    );
    hit["match_count"] = json!(6);
    hit["matches"] = json!([
        m("title", 1, "Checkout timeouts", false, false),
        m("body", 4, "pool timeout after deploy", true, false),
        m("body", 9, "second timeout", false, true),
        m("body", 12, "never shown", false, false),
    ]);
    let mut single = index(ID2, kind, json!([]), false);
    single["match_count"] = json!(1);
    single["matches"] = json!([m("summary", 1, "timeout", false, false)]);
    page(vec![hit, single], 2, 0)
}

#[tokio::test]
async fn search_text_json_toon() {
    for (noun, kind) in KINDS {
        let request = json!({ "patterns": ["timeout"], "limit": 50, "offset": 0 });
        let args = ["context", noun, "search", "timeout"];
        let home = Home::new();

        let server = serve_query(noun, request.clone(), search_page(kind)).await;
        let text = ok(&run(&home, &server, "text", &args), "Searching");
        assert_eq!(
            text,
            [
                format!("{ID}  2026-11-08  [alert:8f3a]  Checkout timeouts"),
                "    Retry storms follow deploys.".into(),
                "    title:1: Checkout timeouts".into(),
                "    body:4: …pool timeout after deploy".into(),
                "    body:9: second timeout…".into(),
                "    (6 matching lines)".into(),
                format!("{ID2}  2026-11-08  [team-wide]  Checkout timeouts"),
                "    Retry storms follow deploys.".into(),
                "    summary:1: timeout".into(),
                String::new(),
                "Showing 1-2 of 2.".into(),
                String::new(),
            ]
            .join("\n")
        );

        let server = serve_query(noun, request.clone(), search_page(kind)).await;
        let out = ok(&run(&home, &server, "json", &args), "Searching");
        assert_eq!(
            serde_json::from_str::<Value>(&out).unwrap(),
            search_page(kind)
        );

        let server = serve_query(noun, request, search_page(kind)).await;
        let out = ok(&run(&home, &server, "toon", &args), "Searching");
        let decoded: Value = toon_format::decode_default(out.trim_end()).unwrap();
        assert_eq!(decoded, search_page(kind));
    }
}

#[tokio::test]
async fn empty_search_in_json() {
    let server = serve_query(
        "memories",
        json!({ "patterns": ["x"], "limit": 50, "offset": 0 }),
        page(vec![], 0, 0),
    )
    .await;
    let home = Home::new();
    let out = ok(
        &run(
            &home,
            &server,
            "json",
            &["context", "memories", "search", "x"],
        ),
        "Searching",
    );
    assert_eq!(
        serde_json::from_str::<Value>(&out).unwrap(),
        page(vec![], 0, 0)
    );
}

fn entry(kind: &str, subjects: Value, source: Value, deleted: bool) -> Value {
    let mut e = index(ID, kind, subjects.clone(), deleted);
    e["updated_by"] = json!({ "user_id": "u-1", "name": "Dana", "credential": "user_token" });
    e["body"] = json!("Check the pool size.\nThen the retries.");
    e["source"] = source;
    e["team_wide"] = json!(subjects.as_array().unwrap().is_empty());
    e["change_note"] = Value::Null;
    e
}

async fn serve_get(noun: &str, revision: Option<&str>, response: Value) -> MockServer {
    let server = MockServer::start().await;
    let mock =
        Mock::given(method("GET")).and(path(format!("/mgmt/api/v1/agent-context/{noun}/{ID}")));
    let mock = match revision {
        Some(r) => mock.and(query_param("revision", r)),
        None => mock.and(query_param_is_missing("revision")),
    };
    mock.respond_with(ResponseTemplate::new(200).set_body_json(response))
        .expect(1)
        .mount(&server)
        .await;
    server
}

#[tokio::test]
async fn get_current_with_subjects_and_source() {
    for (noun, kind) in KINDS {
        let body = entry(
            kind,
            json!([{ "type": "service", "value": "checkout" }, { "type": "host", "value": "web 01" }]),
            json!("incident review 2026-11-08"),
            false,
        );
        let server = serve_get(noun, None, body).await;
        let home = Home::new();
        let out = ok(
            &run(&home, &server, "text", &["context", noun, "get", ID]),
            &format!("Reading {kind}..."),
        );
        assert_eq!(
            out,
            format!(
                "{kind} {ID}  rev 2\n\
                 Title:    Checkout timeouts\n\
                 Summary:  Retry storms follow deploys.\n\
                 Subjects: service:checkout 'host:web 01'\n\
                 Source:   incident review 2026-11-08\n\
                 Created:  2026-11-01\n\
                 Updated:  2026-11-08 by Dana\n\
                 \n\
                 Check the pool size.\nThen the retries.\n"
            )
        );
    }
}

#[tokio::test]
async fn get_revision_team_wide_null_source_deleted() {
    for (noun, kind) in KINDS {
        let body = entry(kind, json!([]), Value::Null, true);
        let home = Home::new();

        let server = serve_get(noun, Some("2"), body.clone()).await;
        let out = ok(
            &run(
                &home,
                &server,
                "text",
                &["context", noun, "get", ID, "--revision", "2"],
            ),
            "Reading",
        );
        assert_eq!(
            out,
            format!(
                "{kind} {ID}  rev 2\n\
                 Title:    Checkout timeouts\n\
                 Summary:  Retry storms follow deploys.\n\
                 Subjects: team-wide\n\
                 Created:  2026-11-01\n\
                 Updated:  2026-11-08 by Dana\n\
                 Deleted:  yes\n\
                 \n\
                 Check the pool size.\nThen the retries.\n"
            )
        );

        let server = serve_get(noun, Some("2"), body.clone()).await;
        let args = ["context", noun, "get", ID, "--revision", "2"];
        let out = ok(&run(&home, &server, "json", &args), "Reading");
        assert_eq!(serde_json::from_str::<Value>(&out).unwrap(), body);

        let server = serve_get(noun, Some("2"), body.clone()).await;
        let out = ok(&run(&home, &server, "toon", &args), "Reading");
        let decoded: Value = toon_format::decode_default(out.trim_end()).unwrap();
        assert_eq!(decoded, body);
    }
}

async fn failing(route: &str, http_method: &str, status: u16, body: Value) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method(http_method))
        .and(path(format!("/mgmt/api/v1/agent-context/{route}")))
        .respond_with(ResponseTemplate::new(status).set_body_json(body))
        .mount(&server)
        .await;
    server
}

fn failed(output: &std::process::Output, message: &str) {
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(stderr(output).contains(message), "{output:?}");
    assert!(stdout(output).is_empty(), "{output:?}");
}

#[tokio::test]
async fn search_pattern_errors_show_service_message() {
    for (noun, _) in KINDS {
        let home = Home::new();
        let server = failing(
            &format!("{noun}/query"),
            "POST",
            400,
            json!({ "code": "invalid_pattern", "message": "unclosed group", "field": "patterns" }),
        )
        .await;
        failed(
            &run(&home, &server, "text", &["context", noun, "search", "("]),
            "patterns: unclosed group",
        );

        let server = failing(
            &format!("{noun}/query"),
            "POST",
            422,
            json!({ "code": "pattern_timeout", "message": "pattern took too long" }),
        )
        .await;
        failed(
            &run(
                &home,
                &server,
                "text",
                &["context", noun, "search", "(a+)+"],
            ),
            "pattern took too long",
        );
    }
}

#[tokio::test]
async fn get_not_found_names_the_entry() {
    for (noun, kind) in KINDS {
        let home = Home::new();
        let server = failing(
            &format!("{noun}/{ID}"),
            "GET",
            404,
            json!({ "code": "not_found", "message": "entry not found" }),
        )
        .await;
        failed(
            &run(&home, &server, "text", &["context", noun, "get", ID]),
            &format!("No such {kind} {ID} in this team (or no such revision)."),
        );
    }
}

#[tokio::test]
async fn invalid_subject_filter_sends_nothing() {
    let server = MockServer::start().await;
    let home = Home::new();
    let output = run(
        &home,
        &server,
        "text",
        &["context", "memories", "list", "--subject", "alert:"],
    );
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(request_count(&server).await, 0);
}

#[tokio::test]
async fn printed_subjects_round_trip_through_a_shell() {
    let subjects = json!([
        { "type": "ip", "value": "2001:db8::" },
        { "type": "host", "value": "web 01" },
        { "type": "service", "value": "a,b" },
        { "type": "dataset", "value": "it's" },
        { "type": "path", "value": r"C:\logs\app" },
        { "type": "alert" },
    ]);
    let listed = page(vec![index(ID, "memory", subjects.clone(), false)], 1, 0);
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(query_path("memories")))
        .and(body_json(json!({ "limit": 50, "offset": 0 })))
        .respond_with(ResponseTemplate::new(200).set_body_json(listed.clone()))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(query_path("memories")))
        .and(body_json(
            json!({ "subjects": subjects, "limit": 50, "offset": 0 }),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(listed))
        .expect(1)
        .mount(&server)
        .await;

    let home = Home::new();
    let out = ok(
        &run(&home, &server, "text", &["context", "memories", "list"]),
        "Listing",
    );
    let first = out.lines().next().unwrap();
    let printed = &first[first.find('[').unwrap() + 1..first.rfind(']').unwrap()];

    let script = format!(
        "set --; for s in {printed}; do set -- \"$@\" --subject \"$s\"; done; \
         exec \"$CX\" --api-key test --region {} -o json context memories list \"$@\"",
        server.uri()
    );
    let output = shell(&home, &script);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
}
