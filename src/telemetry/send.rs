//! OTLP/HTTP JSON encoding and the one POST to the collector.
//!
//! No OTel SDK: the body is the `resourceLogs` JSON the collector's OTLP
//! receiver accepts, built from the flat event JSON. No auth header is sent;
//! the collector path is open and validates every record itself.

use std::time::Duration;

use serde_json::{json, Map, Value};

use super::event::{Event, Resource};

/// Total time a flush may add to a command, connect included.
pub const BUDGET: Duration = Duration::from_millis(400);
/// Records per request.
pub const MAX_BATCH: usize = 50;

/// The OTLP logs request body for `events`, all sharing `resource`.
pub fn encode(resource: &Resource, events: &[Event]) -> Value {
    let resource = match serde_json::to_value(resource) {
        Ok(Value::Object(map)) => map,
        _ => Map::new(),
    };
    json!({
        "resourceLogs": [{
            "resource": { "attributes": attributes(resource) },
            "scopeLogs": [{ "logRecords": events.iter().map(log_record).collect::<Vec<_>>() }]
        }]
    })
}

fn log_record(event: &Event) -> Value {
    let mut map = match serde_json::to_value(event) {
        Ok(Value::Object(map)) => map,
        _ => Map::new(),
    };
    let time = map
        .remove("time_unix_nano")
        .and_then(|t| t.as_u64())
        .unwrap_or_default();
    // OTLP JSON carries 64-bit integers as strings.
    json!({ "timeUnixNano": time.to_string(), "attributes": attributes(map) })
}

/// Flat JSON → OTLP `KeyValue`s. Events hold only strings, bools and
/// integers, so nothing else needs a mapping.
fn attributes(map: Map<String, Value>) -> Vec<Value> {
    map.into_iter()
        .filter_map(|(key, value)| {
            let value = match value {
                Value::String(s) => json!({ "stringValue": s }),
                Value::Bool(b) => json!({ "boolValue": b }),
                Value::Number(n) if n.is_u64() || n.is_i64() => {
                    json!({ "intValue": n.to_string() })
                }
                _ => return None,
            };
            Some(json!({ "key": key, "value": value }))
        })
        .collect()
}

/// POST `body` within [`BUDGET`]. True when the collector answered at all:
/// any HTTP status ends the batch, since retrying a request the collector
/// rejected would only be rejected again. False on a network error or
/// timeout, so the batch is kept for the next flush.
pub async fn post(endpoint: &str, body: &Value) -> bool {
    let Ok(client) = reqwest::Client::builder()
        .user_agent(concat!("cx-cli/", env!("CARGO_PKG_VERSION")))
        .timeout(BUDGET)
        .build()
    else {
        return false;
    };
    client.post(endpoint).json(body).send().await.is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::OutputFormat;
    use crate::telemetry::event::*;
    use uuid::Uuid;

    /// Attribute order is not meaningful in OTLP; sort so bodies compare.
    fn normalize(mut body: Value) -> Value {
        fn sort(attrs: &mut Value) {
            if let Some(a) = attrs.as_array_mut() {
                a.sort_by_key(|kv| kv["key"].as_str().unwrap_or_default().to_string());
            }
        }
        let rl = &mut body["resourceLogs"][0];
        sort(&mut rl["resource"]["attributes"]);
        if let Some(records) = rl["scopeLogs"][0]["logRecords"].as_array_mut() {
            for r in records {
                sort(&mut r["attributes"]);
            }
        }
        body
    }

    /// The example request from contributing/telemetry-design-draft.md § 1.
    const SPEC_EXAMPLE: &str = r#"{
      "resourceLogs": [{
        "resource": { "attributes": [
          { "key": "service.name", "value": { "stringValue": "cx-cli" } },
          { "key": "service.version", "value": { "stringValue": "0.1.32" } },
          { "key": "os.type", "value": { "stringValue": "darwin" } },
          { "key": "host.arch", "value": { "stringValue": "arm64" } },
          { "key": "cx.install.id", "value": { "stringValue": "3f0e6c1a-9b7d-4c55-8e21-5a6f0d2b7c90" } },
          { "key": "cx.install.method", "value": { "stringValue": "brew" } }
        ]},
        "scopeLogs": [{ "logRecords": [{
          "timeUnixNano": "1794480000000000000",
          "attributes": [
            { "key": "event.name", "value": { "stringValue": "cli.command" } },
            { "key": "cx.invocation.id", "value": { "stringValue": "b1d1c0de-2f43-4a0b-9d0e-6c1f5e7a8b90" } },
            { "key": "cx.agent", "value": { "stringValue": "claude_code" } },
            { "key": "cx.ci", "value": { "boolValue": false } },
            { "key": "cx.tty", "value": { "boolValue": false } },
            { "key": "cx.command.path", "value": { "stringValue": "logs" } },
            { "key": "cx.command.flags", "value": { "stringValue": "--limit,--start" } },
            { "key": "cx.outcome", "value": { "stringValue": "success" } },
            { "key": "cx.error.class", "value": { "stringValue": "none" } },
            { "key": "cx.http.status_class", "value": { "stringValue": "2xx" } },
            { "key": "cx.duration_ms", "value": { "intValue": "412" } },
            { "key": "cx.output.format", "value": { "stringValue": "toon" } },
            { "key": "cx.profile.count", "value": { "intValue": "1" } },
            { "key": "cx.spilled", "value": { "boolValue": false } }
          ]
        }]}]
      }]
    }"#;

    #[test]
    fn encodes_the_spec_example() {
        let resource = Resource {
            service_name: "cx-cli",
            service_version: "0.1.32",
            os_type: "darwin",
            host_arch: "arm64",
            install_id: Uuid::parse_str("3f0e6c1a-9b7d-4c55-8e21-5a6f0d2b7c90").unwrap(),
            install_method: "brew",
        };
        let event: Event = serde_json::from_value(json!({
            "time_unix_nano": 1_794_480_000_000_000_000u64,
            "event.name": "cli.command",
            "cx.invocation.id": "b1d1c0de-2f43-4a0b-9d0e-6c1f5e7a8b90",
            "cx.agent": "claude_code",
            "cx.ci": false,
            "cx.tty": false,
            "cx.command.path": "logs",
            "cx.command.flags": "--limit,--start",
            "cx.outcome": "success",
            "cx.error.class": "none",
            "cx.http.status_class": "2xx",
            "cx.duration_ms": 412,
            "cx.output.format": "toon",
            "cx.profile.count": 1,
            "cx.spilled": false
        }))
        .unwrap();

        let expected: Value = serde_json::from_str(SPEC_EXAMPLE).unwrap();
        assert_eq!(normalize(encode(&resource, &[event])), normalize(expected));
    }

    #[test]
    fn team_id_is_a_string_and_one_record_per_event() {
        let id = Uuid::new_v4();
        let events = vec![
            Event::now(id, TeamId::new(4015532), EventKind::FirstRun),
            Event::now(
                id,
                None,
                EventKind::Command(CommandEvent {
                    path: CommandPath::from_names(["schema"]).unwrap(),
                    flags: CommandFlags::default(),
                    outcome: Outcome::Success,
                    error_class: ErrorClass::None,
                    status_class: StatusClass::None,
                    duration_ms: 3,
                    output_format: Some(OutputFormat::Json),
                    profile_count: 0,
                    spilled: false,
                }),
            ),
        ];
        let body = encode(&Resource::current(&id.to_string()), &events);
        let records = body["resourceLogs"][0]["scopeLogs"][0]["logRecords"]
            .as_array()
            .unwrap();
        assert_eq!(records.len(), 2);
        let team = records[0]["attributes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|kv| kv["key"] == "cx.team.id")
            .unwrap();
        assert_eq!(team["value"], json!({ "stringValue": "4015532" }));
        assert!(records[0].get("body").is_none());
    }
}
