#[path = "../common/mod.rs"]
mod common;

use serde_json::json;
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use coralogix_cli::commands::views::{run_folders_list, run_folders_update, run_list};
use coralogix_cli::config::OutputFormat;

#[tokio::test]
async fn list_views_from_mock() {
    let server = MockServer::start().await;

    let body = json!({
        "views": [
            { "id": "v-001", "name": "My View", "folderId": "f-001", "createdAt": "2024-01-01" }
        ]
    });

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/data-exploration/views/v1/views"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    run_list(&[target], OutputFormat::Json)
        .await
        .expect("run_list should succeed");
}

#[tokio::test]
async fn list_views_empty() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/data-exploration/views/v1/views"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "views": [] })))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    run_list(&[target], OutputFormat::Text)
        .await
        .expect("run_list should succeed with empty");
}

#[tokio::test]
async fn list_view_folders_from_mock() {
    let server = MockServer::start().await;

    let body = json!({
        "folders": [
            { "id": "f-001", "name": "Infra" }
        ]
    });

    Mock::given(method("GET"))
        .and(path("/mgmt/openapi/5/data-exploration/views/v1/folders"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let target = common::test_target("test-profile", &server.uri());
    run_folders_list(&[target], OutputFormat::Json)
        .await
        .expect("run_folders_list should succeed");
}

/// `PUT /folders` has no id in the path. The id from the command line must
/// win over the one in the file, so an exported folder can be re-applied
/// to the folder the user named.
#[tokio::test]
async fn folders_update_sends_the_positional_id_in_the_body() {
    let server = MockServer::start().await;

    Mock::given(method("PUT"))
        .and(path("/mgmt/openapi/5/data-exploration/views/v1/folders"))
        .and(body_json(json!({ "id": "f-cli", "name": "Renamed" })))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "folder": { "id": "f-cli", "name": "Renamed" } })),
        )
        .expect(1)
        .mount(&server)
        .await;

    let file = std::env::temp_dir().join(format!(
        "cx_views_folder_update_test_{}.json",
        std::process::id()
    ));
    std::fs::write(
        &file,
        json!({ "id": "f-from-file", "name": "Renamed" }).to_string(),
    )
    .unwrap();

    let target = common::test_target("test-profile", &server.uri());
    let result = run_folders_update(
        &[target],
        "f-cli",
        file.to_str().unwrap(),
        OutputFormat::Json,
    )
    .await;
    let _ = std::fs::remove_file(&file);
    result.expect("run_folders_update should succeed");
}
