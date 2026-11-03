use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::api_client::CxClient;
use crate::error::Result;

const SOURCE_MAPS_BASE: &str = "/mgmt/openapi/5/rum/source-maps/v1";
const MAX_PAGE_SIZE: u32 = 1000;

// --- Response types ---

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceMapRelease {
    #[serde(default)]
    pub application: String,
    #[serde(default)]
    pub version: String,
    pub platform: Option<String>,
    pub upload_status: Option<String>,
    pub commit_hash: Option<String>,
    pub repo_name: Option<String>,
    pub create_time: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListReleasesResponse {
    #[serde(default)]
    pub releases: Vec<SourceMapRelease>,
    #[serde(default)]
    pub next_page_token: String,
}

/// A frame as RUM records it in `error_context.original_stacktrace`.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StackFrame {
    pub file_name: String,
    pub line_number: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column_number: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceCodeLine {
    #[serde(default)]
    pub line_number: u32,
    #[serde(default)]
    pub text: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceLocation {
    #[serde(default)]
    pub file_name: String,
    #[serde(default)]
    pub line_number: u32,
    #[serde(default)]
    pub column_number: u32,
    #[serde(default)]
    pub function_name: String,
    #[serde(default)]
    pub code_lines: Vec<SourceCodeLine>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MappedStackFrame {
    #[serde(default)]
    pub file_name: String,
    #[serde(default)]
    pub line_number: u32,
    pub column_number: Option<u32>,
    pub status: Option<String>,
    pub source: Option<SourceLocation>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MapStackTraceResponse {
    #[serde(default)]
    pub application: String,
    #[serde(default)]
    pub version: String,
    pub commit_hash: Option<String>,
    pub repo_name: Option<String>,
    #[serde(default)]
    pub frames: Vec<MappedStackFrame>,
}

/// Strips a proto enum prefix: `SOURCE_MAP_PLATFORM_WEB` -> `WEB`.
pub fn short_enum(value: Option<&str>, prefix: &str) -> String {
    value
        .map(|v| v.strip_prefix(prefix).unwrap_or(v).to_string())
        .unwrap_or_else(|| "-".to_string())
}

// --- API ---

pub struct RumApi<'a> {
    client: &'a CxClient,
}

impl<'a> RumApi<'a> {
    pub fn new(client: &'a CxClient) -> Self {
        Self { client }
    }

    /// Lists every release, following page tokens until the server stops returning one.
    pub async fn list_releases(&self, application: Option<&str>) -> Result<Vec<SourceMapRelease>> {
        let path = format!("{SOURCE_MAPS_BASE}/releases");
        let page_size = MAX_PAGE_SIZE.to_string();
        let mut releases = Vec::new();
        let mut page_token = String::new();
        loop {
            let mut params = vec![("pageSize", page_size.as_str())];
            if let Some(app) = application {
                params.push(("application", app));
            }
            if !page_token.is_empty() {
                params.push(("pageToken", page_token.as_str()));
            }
            let page: ListReleasesResponse = self.client.get(&path, &params).await?;
            releases.extend(page.releases);
            if page.next_page_token.is_empty() {
                return Ok(releases);
            }
            page_token = page.next_page_token;
        }
    }

    pub async fn map_stack_trace(
        &self,
        application: &str,
        version: &str,
        platform: &str,
        frames: &[StackFrame],
    ) -> Result<MapStackTraceResponse> {
        let body = json!({
            "application": application,
            "version": version,
            "platform": platform,
            "frames": frames,
        });
        self.client
            .post(&format!("{SOURCE_MAPS_BASE}/stack-traces:map"), &body)
            .await
    }
}

/// Parses frames from RUM's `original_stacktrace` JSON: a bare array, or an object with `frames`.
pub fn parse_frames(raw: &str) -> std::result::Result<Vec<StackFrame>, String> {
    let value: Value = serde_json::from_str(raw).map_err(|e| format!("invalid JSON: {e}"))?;
    let frames = match value {
        Value::Object(mut map) => map.remove("frames").unwrap_or(Value::Null),
        other => other,
    };
    serde_json::from_value(frames).map_err(|e| {
        format!("expected an array of {{fileName, lineNumber, columnNumber}} frames: {e}")
    })
}

/// Parses `--frame <url>:<line>[:<column>]`. The URL itself may contain colons.
pub fn parse_frame_arg(raw: &str) -> std::result::Result<StackFrame, String> {
    let error = || format!("invalid frame '{raw}'; expected <file>:<line>[:<column>]");
    let mut parts = raw.rsplitn(3, ':');
    let last: u32 = parts
        .next()
        .and_then(|s| s.parse().ok())
        .ok_or_else(error)?;
    let middle = parts.next().ok_or_else(error)?;
    match (middle.parse::<u32>(), parts.next()) {
        (Ok(line), Some(file)) if !file.is_empty() => Ok(StackFrame {
            file_name: file.to_string(),
            line_number: line,
            column_number: Some(last),
        }),
        _ if !middle.is_empty() => Ok(StackFrame {
            file_name: raw
                .rsplit_once(':')
                .map(|(f, _)| f.to_string())
                .ok_or_else(error)?,
            line_number: last,
            column_number: None,
        }),
        _ => Err(error()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserializes_list_releases() {
        let resp: ListReleasesResponse = serde_json::from_value(json!({
            "releases": [{
                "application": "shop",
                "version": "1.2.3",
                "platform": "SOURCE_MAP_PLATFORM_WEB",
                "uploadStatus": "SOURCE_MAP_UPLOAD_STATUS_SUCCEEDED",
                "commitHash": "abc123",
                "repoName": "org/shop",
                "createTime": "2026-11-01T10:00:00Z"
            }],
            "nextPageToken": "MTAw"
        }))
        .unwrap();
        assert_eq!(resp.releases.len(), 1);
        assert_eq!(resp.releases[0].version, "1.2.3");
        assert_eq!(resp.next_page_token, "MTAw");
        assert_eq!(
            short_enum(resp.releases[0].platform.as_deref(), "SOURCE_MAP_PLATFORM_"),
            "WEB"
        );
    }

    #[test]
    fn deserializes_empty_list_releases() {
        let resp: ListReleasesResponse = serde_json::from_value(json!({})).unwrap();
        assert!(resp.releases.is_empty());
        assert!(resp.next_page_token.is_empty());
    }

    #[test]
    fn deserializes_map_stack_trace() {
        let resp: MapStackTraceResponse = serde_json::from_value(json!({
            "application": "shop",
            "version": "1.2.3",
            "commitHash": "abc123",
            "repoName": "org/shop",
            "frames": [
                {
                    "fileName": "https://shop.example.com/main.js",
                    "lineNumber": 1,
                    "columnNumber": 5,
                    "status": "STACK_FRAME_MAPPING_STATUS_MAPPED",
                    "source": {
                        "fileName": "src/cart.ts",
                        "lineNumber": 42,
                        "columnNumber": 7,
                        "functionName": "addToCart",
                        "codeLines": [{ "lineNumber": 42, "text": "throw new Error()" }]
                    }
                },
                {
                    "fileName": "https://shop.example.com/vendor.js",
                    "lineNumber": 1,
                    "status": "STACK_FRAME_MAPPING_STATUS_SOURCE_MAP_NOT_FOUND"
                }
            ]
        }))
        .unwrap();
        let source = resp.frames[0].source.as_ref().unwrap();
        assert_eq!(source.file_name, "src/cart.ts");
        assert_eq!(source.code_lines[0].line_number, 42);
        assert!(resp.frames[1].source.is_none());
        assert_eq!(resp.frames[1].column_number, None);
    }

    #[test]
    fn parses_rum_original_stacktrace() {
        let frames = parse_frames(
            r#"[{"columnNumber": 398981, "fileName": "https://a.com/main.js", "functionName": "onClick", "lineNumber": 153},
                {"fileName": "https://a.com/main.js", "lineNumber": 2}]"#,
        )
        .unwrap();
        assert_eq!(frames[0].column_number, Some(398981));
        assert_eq!(frames[1].column_number, None);
        assert_eq!(parse_frames(r#"{"frames": []}"#).unwrap(), vec![]);
        assert!(parse_frames(r#"[{"lineNumber": 1}]"#).is_err());
    }

    #[test]
    fn parses_frame_arg() {
        assert_eq!(
            parse_frame_arg("https://a.com:8080/main.js:153:398981").unwrap(),
            StackFrame {
                file_name: "https://a.com:8080/main.js".into(),
                line_number: 153,
                column_number: Some(398981),
            }
        );
        assert_eq!(
            parse_frame_arg("main.js:10").unwrap(),
            StackFrame {
                file_name: "main.js".into(),
                line_number: 10,
                column_number: None,
            }
        );
        assert!(parse_frame_arg("main.js").is_err());
        assert!(parse_frame_arg(":10").is_err());
    }
}
