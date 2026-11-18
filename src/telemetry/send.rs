//! OTLP/HTTP JSON encoding and the sender sub-process, `cx __telemetry-send`,
//! which posts the spool to the collector after the command has exited.
//!
//! No OTel SDK: the body is the `resourceLogs` JSON the collector's OTLP
//! receiver accepts, built from the flat event JSON. No auth header is sent;
//! the collector path is open and validates every record itself.

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{json, Map, Value};

use super::consent::{self, Mode};
use super::event::{Event, Resource};
use super::spool;

/// `argv[1]` of the sender sub-process.
pub const SENDER_ARG: &str = "__telemetry-send";
/// Records per request.
pub const MAX_BATCH: usize = 50;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// The sender exits after this long whatever it is doing. Below the 60 s
/// after which a `.sending` file counts as abandoned.
const LIFETIME: Duration = Duration::from_secs(30);
/// After a kept batch, no sender is started for this long.
const BACKOFF: Duration = Duration::from_secs(15 * 60);

fn sibling(spool: &Path, name: &str) -> PathBuf {
    spool.with_file_name(name)
}

/// The one-sender-at-a-time lock, next to `spool`: an OS advisory lock, freed
/// by the kernel when the holder exits. `None` when another process holds it
/// or it can't be taken at all, in which case nothing is sent.
pub fn lock(spool: &Path) -> Option<File> {
    let file = File::options()
        .create(true)
        .append(true)
        .open(sibling(spool, "telemetry-sender.lock"))
        .ok()?;
    file.try_lock().ok()?;
    Some(file)
}

/// The last sender kept its batch less than [`BACKOFF`] ago.
pub fn backing_off(spool: &Path) -> bool {
    fs::metadata(sibling(spool, "telemetry-backoff"))
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age < BACKOFF)
}

/// The sender sub-process: send the spool in batches of [`MAX_BATCH`] until
/// it is empty or a batch is kept. Reads the consent mode itself, so an
/// opt-out set after the spawn still stops it. Records no telemetry.
pub fn run_sender() {
    let Mode::On { endpoint } = consent::mode() else {
        return;
    };
    let Some(spool) = spool::path() else {
        return;
    };
    let Some(_lock) = lock(&spool) else {
        return;
    };
    // A DNS lookup runs on a blocking thread no request timeout can stop.
    std::thread::spawn(|| {
        std::thread::sleep(LIFETIME);
        std::process::exit(0);
    });
    let _ = rustls::crypto::ring::default_provider().install_default();
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return;
    };
    runtime.block_on(drain(&endpoint, &spool));
}

async fn drain(endpoint: &str, spool: &Path) {
    let resource = Resource::current(&crate::version_cache::installation_id());
    let backoff = sibling(spool, "telemetry-backoff");
    let token = uuid::Uuid::new_v4().to_string();
    loop {
        let mut taken = spool::take(spool, &token);
        let mut batch = std::mem::take(&mut taken.events);
        let mut unsent = batch.split_off(batch.len().min(MAX_BATCH));
        if batch.is_empty() {
            taken.finish(spool, &[]);
            return;
        }
        let done = post(endpoint, &encode(&resource, &batch)).await;
        if !done {
            unsent.extend(batch);
        }
        taken.finish(spool, &unsent);
        if !done {
            let _ = fs::write(&backoff, "");
            return;
        }
        let _ = fs::remove_file(&backoff);
    }
}

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

/// POST `body`. True when the batch is finished: the collector took it (2xx)
/// or rejected it (4xx), and a rejected request would only be rejected again.
/// False when the batch must be kept: a network error or timeout, or a
/// response that does not show the collector looked at it (a redirect, 408,
/// 429, or any 5xx, which a gateway in front of the collector also returns).
async fn post(endpoint: &str, body: &Value) -> bool {
    let Ok(client) = reqwest::Client::builder()
        .user_agent(concat!("cx-cli/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        // A followed redirect re-sends the POST as a bodyless GET, possibly
        // to another host.
        .redirect(reqwest::redirect::Policy::none())
        .build()
    else {
        return false;
    };
    match client.post(endpoint).json(body).send().await {
        Ok(response) => batch_finished(response.status().as_u16()),
        Err(_) => false,
    }
}

fn batch_finished(status: u16) -> bool {
    matches!(status, 200..=299 | 400..=499) && !matches!(status, 408 | 429)
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
        assert!(
            records[1]["attributes"]
                .as_array()
                .unwrap()
                .iter()
                .all(|kv| kv["key"] != "cx.team.id"),
            "no team id means no attribute"
        );
        assert!(records[0].get("body").is_none());
    }
}
