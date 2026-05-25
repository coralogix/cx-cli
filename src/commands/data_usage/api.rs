use serde::Deserialize;
use serde_json::Value;

use crate::error::Result;

use crate::api_client::CxClient;

// --- Response types ---

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataUsageSummaryResponse {
    #[serde(default)]
    pub usage: Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyUsageResponse {
    #[serde(default)]
    pub data: Vec<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogsCountResponse {
    #[serde(default)]
    pub count: Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpansCountResponse {
    #[serde(default)]
    pub count: Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportStatusResponse {
    #[serde(default)]
    pub status: Value,
}

// --- API ---

const DATA_USAGE_BASE: &str = "/mgmt/openapi/5/dataplans/data-usage/v2";

pub struct DataUsageApi<'a> {
    client: &'a CxClient,
}

impl<'a> DataUsageApi<'a> {
    pub fn new(client: &'a CxClient) -> Self {
        Self { client }
    }

    pub async fn get_usage(&self, params: &[(&str, &str)]) -> Result<Value> {
        let raw = self
            .client
            .get_event_stream_raw(DATA_USAGE_BASE, params)
            .await?;
        parse_data_usage_raw_response(&raw)
    }

    pub async fn daily(&self, data_type: &str, body: &Value) -> Result<Value> {
        let path = format!("{DATA_USAGE_BASE}/daily/{data_type}");
        self.client.post(&path, body).await
    }

    pub async fn logs_count(&self, params: &[(&str, &str)]) -> Result<Value> {
        let path = format!("{DATA_USAGE_BASE}/logs/count");
        let raw = self.client.get_event_stream_raw(&path, params).await?;
        parse_data_usage_raw_response(&raw)
    }

    pub async fn spans_count(&self, params: &[(&str, &str)]) -> Result<Value> {
        let path = format!("{DATA_USAGE_BASE}/spans/count");
        let raw = self.client.get_event_stream_raw(&path, params).await?;
        parse_data_usage_raw_response(&raw)
    }

    pub async fn export_status(&self) -> Result<Value> {
        let path = format!("{DATA_USAGE_BASE}/export-status");
        self.client.get(&path, &[]).await
    }
}

pub fn parse_data_usage_raw_response(raw: &str) -> Result<Value> {
    if let Ok(value) = serde_json::from_str(raw) {
        return Ok(value);
    }

    // The Data Usage overview documents `Get data usage`, `logs/count`, and
    // `spans/count` as `Accept: text/event-stream` endpoints that return
    // newline-delimited JSON. Some responses still arrive as one JSON object,
    // so parse that first and fall back to the documented line-delimited form.
    let mut values = Vec::new();
    for line in raw.lines() {
        let mut line = line.trim();
        if line.is_empty() || line.starts_with(':') {
            continue;
        }
        if let Some(data) = line.strip_prefix("data:") {
            line = data.trim();
        }
        if line.is_empty() || line == "[DONE]" {
            continue;
        }

        values.push(serde_json::from_str(line)?);
    }

    Ok(match values.len() {
        0 => Value::Null,
        1 => values.remove(0),
        _ => merge_count_chunks(&values).unwrap_or(Value::Array(values)),
    })
}

fn merge_count_chunks(values: &[Value]) -> Option<Value> {
    for count_key in ["logsCount", "spansCount"] {
        let mut merged = Vec::new();
        let mut matched = false;

        for value in values {
            let items = value
                .get("result")
                .and_then(|result| result.get(count_key))
                .and_then(Value::as_array)?;
            matched = true;
            merged.extend(items.iter().cloned());
        }

        if matched {
            return Some(serde_json::json!({
                "result": {
                    count_key: merged
                }
            }));
        }
    }

    None
}

// --- Tests ---

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn deserialize_summary_response() {
        let json = json!({
            "usage": {
                "totalGb": 42.5,
                "logsGb": 30.0,
                "spansGb": 12.5
            }
        });
        let resp: DataUsageSummaryResponse = serde_json::from_value(json).unwrap();
        assert!(resp.usage.get("totalGb").is_some());
    }

    #[test]
    fn deserialize_daily_response() {
        let json = json!({
            "data": [
                {"date": "2024-01-01", "value": 10.5},
                {"date": "2024-01-02", "value": 12.3}
            ]
        });
        let resp: DailyUsageResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.data.len(), 2);
    }

    #[test]
    fn deserialize_empty_daily() {
        let json = json!({});
        let resp: DailyUsageResponse = serde_json::from_value(json).unwrap();
        assert!(resp.data.is_empty());
    }

    #[test]
    fn deserialize_logs_count_response() {
        let json = json!({ "count": 1000000 });
        let resp: LogsCountResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.count, json!(1000000));
    }

    #[test]
    fn deserialize_spans_count_response() {
        let json = json!({ "count": 500000 });
        let resp: SpansCountResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.count, json!(500000));
    }

    #[test]
    fn parse_data_usage_raw_response_accepts_single_json() {
        let value = parse_data_usage_raw_response(r#"{"count":1000000}"#).unwrap();
        assert_eq!(value, json!({"count": 1000000}));
    }

    #[test]
    fn parse_data_usage_raw_response_accepts_ndjson() {
        let raw = r#"{"timestamp":"2026-05-25T00:00:00Z","count":"1"}
{"timestamp":"2026-05-25T01:00:00Z","count":"2"}
"#;
        let value = parse_data_usage_raw_response(raw).unwrap();
        assert_eq!(
            value,
            json!([
                {"timestamp": "2026-05-25T00:00:00Z", "count": "1"},
                {"timestamp": "2026-05-25T01:00:00Z", "count": "2"}
            ])
        );
    }

    #[test]
    fn parse_data_usage_raw_response_merges_count_chunks() {
        let raw = r#"{"result":{"logsCount":[{"timestamp":"2026-05-25T00:00:00Z","logsCount":"1"}]}}
{"result":{"logsCount":[{"timestamp":"2026-05-25T01:00:00Z","logsCount":"2"}]}}
"#;
        let value = parse_data_usage_raw_response(raw).unwrap();
        assert_eq!(
            value,
            json!({
                "result": {
                    "logsCount": [
                        {"timestamp": "2026-05-25T00:00:00Z", "logsCount": "1"},
                        {"timestamp": "2026-05-25T01:00:00Z", "logsCount": "2"}
                    ]
                }
            })
        );
    }
}
