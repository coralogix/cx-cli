use std::collections::BTreeMap;

use serde_json::{json, Value};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::fixture::{run, stderr, stdout, Home};

const ID: &str = "9a2f6c1e-0000-4000-8000-000000000001";
const KINDS: [(&str, &str); 2] = [("memories", "memory"), ("instructions", "instruction")];
const TEAM_PATH: &str = "/mgmt/api/v1/agent-context/history";

fn revision(rev: u64, op: &str, reverted_to: Option<u64>, name: Option<&str>) -> Value {
    json!({
        "revision": rev, "operation": op, "reverted_to": reverted_to,
        "change_note": if op == "update" { json!("bumped pool size") } else { Value::Null },
        "author": { "user_id": null, "name": name, "credential": "team_api_key" },
        "client": "cx", "created_at": format!("2026-11-08T10:00:0{rev}.5Z"),
        "title": "Checkout timeouts", "deleted": op == "delete",
    })
}

fn team_revision(rev: u64, op: &str, kind: &str) -> Value {
    let mut v = revision(rev, op, None, Some("Dana"));
    v["id"] = json!(ID);
    v["kind"] = json!(kind);
    v["current_revision"] = json!(4);
    v
}

fn page(items: Vec<Value>, total: u64, offset: u64) -> Value {
    json!({ "items": items, "total": total, "limit": 50, "offset": offset })
}

async fn serve(route: &str, response: Value) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(route))
        .respond_with(ResponseTemplate::new(200).set_body_json(response))
        .expect(1)
        .mount(&server)
        .await;
    server
}

async fn query_of(server: &MockServer) -> BTreeMap<String, String> {
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    requests[0].url.query_pairs().into_owned().collect()
}

fn ok(output: &std::process::Output, status: &str) -> String {
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(stderr(output).contains(status), "{output:?}");
    stdout(output)
}

fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

#[tokio::test]
async fn team_history_query_strings() {
    let cases: [(&[&str], BTreeMap<String, String>); 4] = [
        (&[], map(&[("limit", "50"), ("offset", "0")])),
        (
            &[
                "--author",
                "Dana Lee",
                "--source",
                "incident review",
                "--kind",
                "instruction",
                "--since",
                "2026-11-01T00:00:00Z",
                "--limit",
                "5",
                "--offset",
                "10",
            ],
            map(&[
                ("author", "Dana Lee"),
                ("source", "incident review"),
                ("kind", "instruction"),
                ("since", "2026-11-01T00:00:00.000Z"),
                ("limit", "5"),
                ("offset", "10"),
            ]),
        ),
        (
            &["--kind", "memory"],
            map(&[("kind", "memory"), ("limit", "50"), ("offset", "0")]),
        ),
        (
            &["--author", "u-1&x=y"],
            map(&[("author", "u-1&x=y"), ("limit", "50"), ("offset", "0")]),
        ),
    ];
    for (flags, expected) in cases {
        let server = serve(TEAM_PATH, page(vec![], 0, 0)).await;
        let home = Home::new();
        let mut args = vec!["context", "history"];
        args.extend(flags);
        let out = ok(&run(&home, &server, "text", &args), "Reading history...");
        assert_eq!(out, "No changes found.\n");
        assert_eq!(query_of(&server).await, expected, "{flags:?}");
    }
}

#[tokio::test]
async fn team_history_relative_since_is_sent_as_rfc3339() {
    let server = serve(TEAM_PATH, page(vec![], 0, 0)).await;
    let home = Home::new();
    let before = chrono::Utc::now() - chrono::Duration::days(7);
    let out = run(
        &home,
        &server,
        "text",
        &["context", "history", "--since", "now-7d"],
    );
    let after = chrono::Utc::now() - chrono::Duration::days(7);
    ok(&out, "Reading history...");
    let since = query_of(&server).await["since"].clone();
    let parsed = chrono::DateTime::parse_from_rfc3339(&since).unwrap();
    let slack = chrono::Duration::seconds(1);
    assert!(
        parsed >= before - slack && parsed <= after + slack,
        "{since}"
    );
}

#[tokio::test]
async fn team_history_bad_since_sends_nothing() {
    let server = MockServer::start().await;
    let home = Home::new();
    let out = run(
        &home,
        &server,
        "text",
        &["context", "history", "--since", "yesterday-ish"],
    );
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn team_history_text_json_toon() {
    let response = page(
        vec![
            team_revision(3, "delete", "memory"),
            team_revision(2, "update", "instruction"),
        ],
        5,
        0,
    );
    let home = Home::new();
    let args = ["context", "history"];

    let server = serve(TEAM_PATH, response.clone()).await;
    let out = ok(&run(&home, &server, "text", &args), "Reading history...");
    let header = out.lines().nth(1).unwrap();
    for column in [
        "Time (UTC)",
        "ID",
        "Kind",
        "Rev",
        "Operation",
        "Author",
        "Title",
    ] {
        assert!(header.contains(column), "{header}");
    }
    assert!(header.contains("Change note"), "{header}");
    assert!(!header.contains("Profile"), "{header}");
    let delete_row = out.lines().find(|l| l.contains("| delete")).unwrap();
    assert!(delete_row.contains("2026-11-08 10:00:03"), "{delete_row}");
    assert!(delete_row.contains(ID) && delete_row.contains("memory"));
    assert!(out.contains("bumped pool size"), "{out}");
    assert!(
        out.ends_with("\n\nShowing 1-2 of 5. More: --offset 2\n"),
        "{out}"
    );

    let server = serve(TEAM_PATH, response.clone()).await;
    let out = ok(&run(&home, &server, "json", &args), "Reading history...");
    assert_eq!(serde_json::from_str::<Value>(&out).unwrap(), response);

    let server = serve(TEAM_PATH, response.clone()).await;
    let out = ok(&run(&home, &server, "toon", &args), "Reading history...");
    let decoded: Value = toon_format::decode_default(out.trim_end()).unwrap();
    assert_eq!(decoded, response);
}

#[tokio::test]
async fn team_history_past_the_end() {
    let server = serve(TEAM_PATH, page(vec![], 7, 50)).await;
    let home = Home::new();
    let out = ok(
        &run(
            &home,
            &server,
            "text",
            &["context", "history", "--offset", "50"],
        ),
        "Reading history...",
    );
    assert_eq!(out, "No changes at this offset; there are 7.\n");
}

#[tokio::test]
async fn entry_history_requests_and_text() {
    for (noun, kind) in KINDS {
        let route = format!("/mgmt/api/v1/agent-context/{noun}/{ID}/revisions");
        let response = page(
            vec![
                revision(4, "revert", Some(2), Some("Dana")),
                revision(3, "delete", None, None),
                revision(2, "update", None, Some("Lee")),
            ],
            4,
            0,
        );
        let server = serve(&route, response).await;
        let home = Home::new();
        let out = ok(
            &run(
                &home,
                &server,
                "text",
                &[
                    "context", noun, "history", ID, "--limit", "3", "--offset", "0",
                ],
            ),
            &format!("Reading {kind} history..."),
        );
        assert_eq!(
            query_of(&server).await,
            map(&[("limit", "3"), ("offset", "0")])
        );
        let header = out.lines().nth(1).unwrap();
        assert!(
            !header.contains("ID") && !header.contains("Kind"),
            "{header}"
        );
        assert!(out.contains("revert to 2"), "{out}");
        let delete_row = out.lines().find(|l| l.contains("| delete")).unwrap();
        assert!(delete_row.contains("team_api_key"), "{delete_row}");
        assert!(
            out.ends_with("\n\nShowing 1-3 of 4. More: --offset 3\n"),
            "{out}"
        );
    }
}

#[tokio::test]
async fn entry_history_json_toon_and_empty_pages() {
    for (noun, kind) in KINDS {
        let route = format!("/mgmt/api/v1/agent-context/{noun}/{ID}/revisions");
        let response = page(vec![revision(1, "create", None, Some("Dana"))], 1, 0);
        let home = Home::new();
        let args = ["context", noun, "history", ID];
        let status = format!("Reading {kind} history...");

        let server = serve(&route, response.clone()).await;
        let out = ok(&run(&home, &server, "json", &args), &status);
        assert_eq!(serde_json::from_str::<Value>(&out).unwrap(), response);

        let server = serve(&route, response.clone()).await;
        let out = ok(&run(&home, &server, "toon", &args), &status);
        let decoded: Value = toon_format::decode_default(out.trim_end()).unwrap();
        assert_eq!(decoded, response);

        let server = serve(&route, page(vec![], 3, 20)).await;
        let out = ok(
            &run(
                &home,
                &server,
                "text",
                &["context", noun, "history", ID, "--offset", "20"],
            ),
            &status,
        );
        assert_eq!(out, "No changes at this offset; there are 3.\n");
    }
}

#[tokio::test]
async fn entry_history_not_found_names_the_entry() {
    for (noun, kind) in KINDS {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(404)
                    .set_body_json(json!({ "code": "not_found", "message": "no such entry" })),
            )
            .mount(&server)
            .await;
        let home = Home::new();
        let out = run(&home, &server, "text", &["context", noun, "history", ID]);
        assert_eq!(out.status.code(), Some(1), "{out:?}");
        assert!(
            stderr(&out).contains(&format!(
                "No such {kind} {ID} in this team (or no such revision)."
            )),
            "{out:?}"
        );
    }
}
