#[path = "../common/mod.rs"]
mod common;

use prost::Message;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use coralogix_cli::commands::datasets;
use coralogix_cli::config::OutputFormat;

#[derive(Clone, PartialEq, Message)]
struct GetSystemDatasetsResponse {
    #[prost(message, repeated, tag = "1")]
    datasets: Vec<SystemDataset>,
}

#[derive(Clone, PartialEq, Message)]
struct SystemDataset {
    #[prost(int32, tag = "1")]
    company_id: i32,
    #[prost(string, tag = "3")]
    dataset: String,
    #[prost(bool, tag = "4")]
    ingestion_enabled: bool,
    #[prost(bool, tag = "7")]
    query_enabled: bool,
    #[prost(string, tag = "8")]
    description: String,
}

#[derive(Clone, PartialEq, Message)]
struct GetUserDefinedDatasetsResponse {
    #[prost(message, repeated, tag = "1")]
    datasets: Vec<UserDefinedDataset>,
}

#[derive(Clone, PartialEq, Message)]
struct UserDefinedDataset {
    #[prost(int32, tag = "1")]
    company_id: i32,
    #[prost(message, optional, tag = "2")]
    dataset: Option<DatasetId>,
    #[prost(bool, tag = "6")]
    write_enabled: bool,
}

#[derive(Clone, PartialEq, Message)]
struct DatasetId {
    #[prost(message, optional, tag = "1")]
    dataspace: Option<Dataspace>,
    #[prost(string, tag = "2")]
    dataset: String,
}

#[derive(Clone, PartialEq, Message)]
struct Dataspace {
    #[prost(string, tag = "1")]
    dataspace: String,
}

fn grpc_web_ok(message: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();
    body.push(0);
    body.extend_from_slice(&(message.len() as u32).to_be_bytes());
    body.extend_from_slice(message);
    let trailers = b"grpc-status: 0\r\ngrpc-message: \r\n";
    body.push(0x80);
    body.extend_from_slice(&(trailers.len() as u32).to_be_bytes());
    body.extend_from_slice(trailers);
    body
}

async fn mount_dataset_mocks(server: &MockServer) {
    let system = GetSystemDatasetsResponse {
        datasets: vec![SystemDataset {
            company_id: 1,
            dataset: "labs.cases.state_updates".into(),
            ingestion_enabled: true,
            query_enabled: true,
            description: "Case lifecycle events".into(),
        }],
    }
    .encode_to_vec();

    let user = GetUserDefinedDatasetsResponse {
        datasets: vec![UserDefinedDataset {
            company_id: 1,
            dataset: Some(DatasetId {
                dataspace: Some(Dataspace {
                    dataspace: "default".into(),
                }),
                dataset: "my_custom".into(),
            }),
            write_enabled: true,
        }],
    }
    .encode_to_vec();

    Mock::given(method("POST"))
        .and(path(
            "/com.coralogix.archive.dataset.v2.SystemDatasetService/GetSystemDatasets",
        ))
        .and(header("content-type", "application/grpc-web+proto"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "application/grpc-web+proto")
                .set_body_bytes(grpc_web_ok(&system)),
        )
        .expect(1)
        .mount(server)
        .await;

    Mock::given(method("POST"))
        .and(path(
            "/com.coralogix.archive.dataset.v2.UserDefinedDatasetService/GetUserDefinedDatasets",
        ))
        .and(header("content-type", "application/grpc-web+proto"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "application/grpc-web+proto")
                .set_body_bytes(grpc_web_ok(&user)),
        )
        .expect(1)
        .mount(server)
        .await;
}

#[tokio::test]
async fn datasets_list_json() {
    let server = MockServer::start().await;
    mount_dataset_mocks(&server).await;

    let target = common::test_target("test-profile", &server.uri());
    datasets::run_list(&[target], OutputFormat::Json)
        .await
        .expect("datasets list should succeed");
}

#[tokio::test]
async fn datasets_list_text() {
    let server = MockServer::start().await;
    mount_dataset_mocks(&server).await;

    let target = common::test_target("test-profile", &server.uri());
    datasets::run_list(&[target], OutputFormat::Text)
        .await
        .expect("datasets list text should succeed");
}

#[tokio::test]
async fn datasets_list_multi_profile() {
    let server = MockServer::start().await;

    let system = GetSystemDatasetsResponse {
        datasets: vec![SystemDataset {
            company_id: 1,
            dataset: "engine.queries".into(),
            ingestion_enabled: true,
            query_enabled: true,
            description: "Queries".into(),
        }],
    }
    .encode_to_vec();
    let user = GetUserDefinedDatasetsResponse { datasets: vec![] }.encode_to_vec();

    Mock::given(method("POST"))
        .and(path(
            "/com.coralogix.archive.dataset.v2.SystemDatasetService/GetSystemDatasets",
        ))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "application/grpc-web+proto")
                .set_body_bytes(grpc_web_ok(&system)),
        )
        .expect(2)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path(
            "/com.coralogix.archive.dataset.v2.UserDefinedDatasetService/GetUserDefinedDatasets",
        ))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "application/grpc-web+proto")
                .set_body_bytes(grpc_web_ok(&user)),
        )
        .expect(2)
        .mount(&server)
        .await;

    let t1 = common::test_target("p1", &server.uri());
    let t2 = common::test_target("p2", &server.uri());
    datasets::run_list(&[t1, t2], OutputFormat::Json)
        .await
        .expect("multi-profile datasets list should succeed");
}
