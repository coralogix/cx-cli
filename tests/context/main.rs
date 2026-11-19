#[path = "../common/mod.rs"]
mod common;
mod create;
mod entries;
mod fixture;
mod get;
mod history;
mod subjects;
mod update;

use fixture::{completion_candidates, cx, request_count, run, stderr, Home};
use serde_json::Value;
use wiremock::MockServer;

const MULTI_PROFILE: &str =
    "cx context does not support multi-profile. Use a single profile with -p <profile>.";

const HELP_LINE: &str = "\n  context ";

fn root_help(home: &Home, flag: bool) -> String {
    let output = cx(home, flag).arg("--help").output().unwrap();
    assert!(output.status.success(), "{output:?}");
    fixture::stdout(&output)
}

fn schema_names(home: &Home, flag: bool) -> Vec<String> {
    let output = cx(home, flag).arg("schema").output().unwrap();
    assert!(output.status.success(), "{output:?}");
    let schema: Value = serde_json::from_slice(&output.stdout).unwrap();
    schema["commands"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["name"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn flag_off_hides_context() {
    let home = Home::new();
    assert!(!root_help(&home, false).contains(HELP_LINE));
    assert!(!schema_names(&home, false).iter().any(|n| n == "context"));
    assert!(!completion_candidates(&home, false, "")
        .iter()
        .any(|c| c == "context"));
}

#[test]
fn flag_on_shows_context() {
    let home = Home::new();
    let help = root_help(&home, true);
    let agent = help.find("Agent:").unwrap();
    let local = help.find("Local:").unwrap();
    let line = help.find(HELP_LINE).expect("context line in root help");
    assert!(agent < line && line < local, "{help}");
    assert!(schema_names(&home, true).iter().any(|n| n == "context"));
    assert!(completion_candidates(&home, true, "")
        .iter()
        .any(|c| c == "context"));
}

#[test]
fn flag_off_keeps_root_command_count() {
    let home = Home::new();
    assert_eq!(
        schema_names(&home, false).len() + 1,
        schema_names(&home, true).len()
    );
}

async fn assert_multi_profile_refused(extra: &[&str]) {
    let server = MockServer::start().await;
    let home = Home::new();
    home.write_profile("a", &server.uri());
    home.write_profile("b", &server.uri());
    let output = cx(&home, false)
        .args(extra)
        .args(["-o", "text", "context", "get"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(stderr(&output).contains(MULTI_PROFILE), "{output:?}");
    assert_eq!(request_count(&server).await, 0);
}

#[tokio::test]
async fn multi_profile_refused_for_two_profiles() {
    assert_multi_profile_refused(&["-p", "a", "-p", "b"]).await;
}

#[tokio::test]
async fn multi_profile_refused_for_repeated_name() {
    assert_multi_profile_refused(&["-p", "a", "-p", "a"]).await;
}

#[tokio::test]
async fn multi_profile_refused_with_missing_profile() {
    assert_multi_profile_refused(&["-p", "a", "-p", "missing"]).await;
}

#[tokio::test]
async fn multi_profile_refused_before_override_check() {
    assert_multi_profile_refused(&[
        "-p",
        "a",
        "-p",
        "b",
        "--api-key",
        "k",
        "--region",
        "http://127.0.0.1:9",
    ])
    .await;
}

#[tokio::test]
async fn invalid_numbers_are_usage_errors() {
    let server = MockServer::start().await;
    let home = Home::new();
    let cases: &[&[&str]] = &[
        &["context", "get", "--limit", "0"],
        &["context", "get", "--limit", "201"],
        &["context", "get", "--limit=-1"],
        &["context", "get", "--limit", "99999999999999999999"],
        &["context", "history", "--offset=-1"],
        &[
            "context",
            "memories",
            "list",
            "--offset",
            "99999999999999999999999",
        ],
        &["context", "instructions", "search", "x", "--limit", "0"],
        &["context", "memories", "history", "id", "--limit", "201"],
        &["context", "memories", "get", "id", "--revision", "0"],
        &[
            "context",
            "memories",
            "delete",
            "id",
            "--expected-revision",
            "0",
        ],
        &[
            "context",
            "instructions",
            "revert",
            "id",
            "--to-revision",
            "0",
            "--expected-revision",
            "1",
        ],
        &[
            "context",
            "instructions",
            "update",
            "id",
            "--expected-revision",
            "0",
        ],
    ];
    for args in cases {
        let output = run(&home, &server, "text", args);
        assert_eq!(output.status.code(), Some(2), "{args:?}: {output:?}");
    }
    assert_eq!(request_count(&server).await, 0);
}

#[tokio::test]
async fn limit_bounds_parse() {
    let server = MockServer::start().await;
    let home = Home::new();
    for limit in ["1", "200"] {
        let output = run(
            &home,
            &server,
            "text",
            &["context", "get", "--limit", limit],
        );
        assert_ne!(output.status.code(), Some(2), "{limit}: {output:?}");
    }
}

#[tokio::test]
async fn scope_flags_conflict() {
    let server = MockServer::start().await;
    let home = Home::new();
    let cases: &[&[&str]] = &[
        &[
            "context",
            "memories",
            "list",
            "--team-wide",
            "--subject",
            "a:b",
        ],
        &[
            "context",
            "memories",
            "search",
            "p",
            "--team-wide",
            "--subject",
            "a:b",
        ],
        &[
            "context",
            "memories",
            "create",
            "--title",
            "t",
            "--summary",
            "s",
            "--body-file",
            "-",
            "--team-wide",
            "--subject",
            "a:b",
        ],
        &[
            "context",
            "instructions",
            "update",
            "id",
            "--expected-revision",
            "1",
            "--team-wide",
            "--subject",
            "a:b",
        ],
        &[
            "context",
            "memories",
            "update",
            "id",
            "--expected-revision",
            "1",
            "--source",
            "s",
            "--no-source",
        ],
        &[
            "context",
            "instructions",
            "create",
            "--title",
            "t",
            "--summary",
            "s",
            "--body-file",
            "-",
        ],
        &["context", "memories", "search"],
        &["context", "memories", "search", "-F"],
    ];
    for args in cases {
        let output = run(&home, &server, "text", args);
        assert_eq!(output.status.code(), Some(2), "{args:?}: {output:?}");
    }
    assert_eq!(request_count(&server).await, 0);
}
