use std::fmt;
use std::io::{self, Read};

use async_trait::async_trait;
use bytes::Bytes;
use flate2::read::GzDecoder;
use http::{Request, Response, StatusCode};
use opentelemetry_http::{HttpClient, HttpError};
use opentelemetry_otlp::{Compression, Protocol, SpanExporter, WithExportConfig, WithHttpConfig};
use opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceRequest;
use opentelemetry_sdk::{trace::SpanExporter as _, Resource};
use prost::Message;
use reqwest::{header, Client, Url};

use crate::limits::{MAX_BATCH_BYTES, MAX_SPANS, MAX_SPAN_BYTES};
use crate::{inline_client, otlp, Config, Delivery, Spool};

// Allow the full span buffer to reach the fitter, plus protobuf framing and
// resource/scope metadata. Only a fitted, 512 KiB batch can go out or to disk.
const MAX_EXPORT_BYTES: usize = MAX_SPANS * MAX_SPAN_BYTES + MAX_BATCH_BYTES;

pub(crate) fn build(config: Config, resource: &Resource) -> Option<SpanExporter> {
    let timeout = config.export_timeout;
    let transport = match config.delivery {
        Delivery::Inline => Transport::Inline {
            client: inline_client(timeout)?,
            endpoint: config.endpoint,
            headers: config.headers,
            #[cfg(feature = "command-failure-logs")]
            failure_logs_enabled: config.command_failure_logs,
            #[cfg(feature = "command-failure-logs")]
            timeout,
        },
        Delivery::Spool(spool) => Transport::Spool(spool),
    };
    let mut exporter = SpanExporter::builder()
        .with_http()
        .with_protocol(Protocol::HttpBinary)
        .with_endpoint("http://localhost/v1/traces")
        .with_timeout(timeout)
        // Upstream has no explicit uncompressed setting. A configured gzip
        // wins over ambient compression, including invalid values. The private
        // adapter normalizes its body before sending it or writing it to disk.
        .with_compression(Compression::Gzip)
        .with_http_client(transport)
        .build()
        .ok()?;
    exporter.set_resource(resource);
    Some(exporter)
}

enum Transport {
    Inline {
        client: Client,
        endpoint: Url,
        headers: header::HeaderMap,
        #[cfg(feature = "command-failure-logs")]
        failure_logs_enabled: bool,
        #[cfg(feature = "command-failure-logs")]
        timeout: std::time::Duration,
    },
    Spool(Spool),
}

impl fmt::Debug for Transport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("Transport").finish_non_exhaustive()
    }
}

#[async_trait]
impl HttpClient for Transport {
    async fn send_bytes(&self, request: Request<Bytes>) -> Result<Response<Bytes>, HttpError> {
        // The upstream HTTP builder merges ambient headers. Keep only its
        // serialized body, and build the real request from explicit settings.
        let payload = decode_payload(&request.into_body())?;
        let status = match self {
            Self::Inline {
                client,
                endpoint,
                headers,
                #[cfg(feature = "command-failure-logs")]
                failure_logs_enabled,
                #[cfg(feature = "command-failure-logs")]
                timeout,
            } => {
                #[cfg(feature = "command-failure-logs")]
                let logs = if *failure_logs_enabled {
                    crate::command_failure_logs_endpoint(endpoint).and_then(|endpoint| {
                        Some((endpoint, crate::encode_command_failure_logs(&payload)?))
                    })
                } else {
                    None
                };
                let traces = post(client, endpoint.clone(), headers, payload);
                #[cfg(feature = "command-failure-logs")]
                if let Some((endpoint, payload)) = logs {
                    let logs = post(client, endpoint, headers, payload);
                    let (traces, logs) =
                        tokio::time::timeout(*timeout, async { tokio::join!(traces, logs) })
                            .await?;
                    let traces = traces?;
                    let status = if traces.is_success() { logs? } else { traces };
                    return response(status);
                }
                traces.await?
            }
            Self::Spool(spool) => {
                spool.enqueue(&payload)?;
                StatusCode::ACCEPTED
            }
        };
        response(status)
    }
}

fn response(status: StatusCode) -> Result<Response<Bytes>, HttpError> {
    Ok(Response::builder().status(status).body(Bytes::new())?)
}

fn decode_payload(body: &[u8]) -> io::Result<Vec<u8>> {
    let mut decoded = Vec::new();
    GzDecoder::new(body)
        .take((MAX_EXPORT_BYTES + 1) as u64)
        .read_to_end(&mut decoded)?;
    if decoded.len() > MAX_EXPORT_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "oversize OTLP export",
        ));
    }
    let invalid = || io::Error::new(io::ErrorKind::InvalidData, "invalid OTLP export");
    let request = ExportTraceServiceRequest::decode(decoded.as_slice()).map_err(|_| invalid())?;
    if otlp::valid_request(&request) {
        Ok(decoded)
    } else {
        otlp::encode(request).ok_or_else(invalid)
    }
}

async fn post(
    client: &Client,
    endpoint: Url,
    headers: &header::HeaderMap,
    payload: Vec<u8>,
) -> Result<StatusCode, reqwest::Error> {
    let response = client
        .post(endpoint)
        .headers(headers.clone())
        .header(header::CONTENT_TYPE, "application/x-protobuf")
        .body(payload)
        .send()
        .await
        .map_err(reqwest::Error::without_url)?;
    Ok(response.status())
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::write::GzEncoder;
    use std::io::Write;

    fn gzip(payload: &[u8]) -> Vec<u8> {
        let mut gzip = GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gzip.write_all(payload).unwrap();
        gzip.finish().unwrap()
    }

    #[test]
    fn rejects_invalid_gzip_protobuf_and_oversize_decompression() {
        assert!(decode_payload(b"not gzip").is_err());
        assert!(decode_payload(&gzip(b"not protobuf")).is_err());
        let oversized = gzip(&vec![0; MAX_EXPORT_BYTES + 1]);
        let error = decode_payload(&oversized).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(error.to_string(), "oversize OTLP export");
    }
}
