//! OTLP/HTTP JSON encoding and the sender sub-process, `cx __telemetry-send`,
//! which posts the spool to Coralogix after the command has exited.
//!
//! No OTel SDK: the body is the `resourceLogs` JSON the Coralogix ingress
//! accepts on `/v1/logs`, built from the flat event JSON. It is sent with the
//! send-your-data key the onboarding service hands out (`key.rs`). Nothing on
//! the server checks the records: the types in `event.rs` are the only limit
//! on what is sent.

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{json, Map, Value};

use super::consent::{self, Mode};
use super::event::{Event, Resource};
use super::key::{self, TelemetryKey};
use super::spool;

/// `argv[1]` of the sender sub-process.
pub const SENDER_ARG: &str = "__telemetry-send";
/// Records per request.
pub const MAX_BATCH: usize = 50;
/// Request body cap in bytes: small requests, far below the ingress limit.
pub const MAX_BODY_BYTES: usize = 60 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// The sender exits after this long whatever it is doing. Below the 60 s
/// after which a `.sending` file counts as abandoned.
const LIFETIME: Duration = Duration::from_secs(30);
/// After a kept batch or a failed key fetch, no sender is started for this
/// long.
const BACKOFF: Duration = Duration::from_secs(15 * 60);

fn sibling(spool: &Path, name: &str) -> PathBuf {
    spool.with_file_name(name)
}

/// The one-sender-at-a-time lock, next to `spool`: an OS advisory lock, freed
/// by the kernel when the holder exits. `None` when another process holds it
/// or it can't be taken at all, in which case nothing is sent.
pub fn lock(spool: &Path) -> Option<File> {
    // With read access: Windows cannot lock a file opened only for append.
    let file = File::options()
        .create(true)
        .read(true)
        .append(true)
        .open(sibling(spool, "telemetry-sender.lock"))
        .ok()?;
    file.try_lock().ok()?;
    Some(file)
}

/// The last sender kept its batch or got no key less than [`BACKOFF`] ago.
pub fn backing_off(spool: &Path) -> bool {
    fs::metadata(sibling(spool, "telemetry-backoff"))
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age < BACKOFF)
}

/// The sender sub-process: send the spool in batches of up to [`MAX_BATCH`]
/// events and [`MAX_BODY_BYTES`] bytes until it is empty or a batch is kept. Reads the consent mode itself, so an
/// opt-out set after the spawn still stops it. Records no telemetry.
pub fn run_sender() {
    let Mode::On { key_url } = consent::mode() else {
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
    runtime.block_on(drain(&key_url, &spool));
}

async fn drain(key_url: &str, spool: &Path) {
    let backoff = sibling(spool, "telemetry-backoff");
    let cache = sibling(spool, "telemetry-key.json");
    let Ok(client) = reqwest::Client::builder()
        .user_agent(concat!("cx-cli/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        // A followed redirect re-sends the POST as a bodyless GET, possibly
        // to another host.
        .redirect(reqwest::redirect::Policy::none())
        .build()
    else {
        return;
    };
    let fetch = || async {
        let key = key::fetch(&client, key_url).await?;
        key::save(&cache, &key);
        Some(key)
    };
    let cached = key::load(&cache);
    // No key (route not deployed, no key stored yet, rate limited, offline):
    // the events wait in the spool.
    let Some(mut key) = (match cached {
        Some(key) => Some(key),
        None => fetch().await,
    }) else {
        let _ = fs::write(&backoff, "");
        return;
    };
    let resource = Resource::current(&crate::version_cache::installation_id());
    let token = uuid::Uuid::new_v4().to_string();
    let mut refetched = false;
    loop {
        let mut taken = spool::take(spool, &token);
        let mut unsent = std::mem::take(&mut taken.events);
        let batch = take_batch(&resource, &mut unsent);
        if batch.is_empty() {
            taken.finish(spool, &[]);
            return;
        }
        let body = encode(&resource, &batch);
        let mut status = post(&client, &key, &body).await;
        // The key was probably rotated: get the new one and try once more,
        // once per sender.
        if key_refused(status) && !refetched {
            refetched = true;
            let _ = fs::remove_file(&cache);
            if let Some(new) = fetch().await {
                key = new;
                status = post(&client, &key, &body).await;
            }
        }
        let done = status.is_some_and(batch_finished);
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

/// Remove and return the oldest events that fit one request: at most
/// [`MAX_BATCH`] of them and at most [`MAX_BODY_BYTES`] encoded. An event too
/// large to fit a request on its own is dropped from `events`, so it cannot
/// block the ones behind it. Empty only when nothing sendable is left.
fn take_batch(resource: &Resource, events: &mut Vec<Event>) -> Vec<Event> {
    // Compact JSON, as `post` sends it: the body is the envelope plus the
    // records and a comma between them (counted here as one per record).
    let envelope = encode(resource, &[]).to_string().len();
    let record_len = |event: &Event| log_record(event).to_string().len() + 1;
    events.retain(|event| envelope + record_len(event) <= MAX_BODY_BYTES);
    let mut size = envelope;
    let count = events
        .iter()
        .take(MAX_BATCH)
        .take_while(|event| {
            size += record_len(event);
            size <= MAX_BODY_BYTES
        })
        .count();
    let rest = events.split_off(count);
    std::mem::replace(events, rest)
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

/// POST `body` to the ingress with `key`. The status, or `None` for a
/// network error or timeout.
async fn post(client: &reqwest::Client, key: &TelemetryKey, body: &Value) -> Option<u16> {
    let request = client
        .post(key.logs_url())
        .bearer_auth(key.bearer())
        .json(body);
    Some(request.send().await.ok()?.status().as_u16())
}

fn key_refused(status: Option<u16>) -> bool {
    matches!(status, Some(401 | 403))
}

/// True when the batch is finished: the ingress took it (2xx) or rejected it
/// (4xx), and a rejected request would only be rejected again. False when the
/// batch must be kept: the key was refused (401, 403), or the response does
/// not show the ingress looked at the batch (a redirect, 408, 429, any 5xx).
fn batch_finished(status: u16) -> bool {
    matches!(status, 200..=299 | 400..=499) && !matches!(status, 401 | 403 | 408 | 429)
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
          { "key": "cx.application.name", "value": { "stringValue": "cx-cli" } },
          { "key": "cx.subsystem.name", "value": { "stringValue": "usage" } },
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
            application_name: "cx-cli",
            subsystem_name: "usage",
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

    fn command(flags: CommandFlags) -> Event {
        Event::now(
            Uuid::new_v4(),
            TeamId::new(4015532),
            EventKind::Command(CommandEvent {
                path: CommandPath::from_names(["dashboards", "query-search"]).unwrap(),
                flags,
                outcome: Outcome::Success,
                error_class: ErrorClass::None,
                status_class: StatusClass::Success2xx,
                duration_ms: 123_456,
                output_format: Some(OutputFormat::Json),
                profile_count: 50,
                spilled: true,
            }),
        )
    }

    #[test]
    fn fifty_maximum_size_events_go_out_in_requests_under_the_body_cap() {
        let names: Vec<String> = (0..30).map(|i| format!("{i:a>40}")).collect();
        let flags = CommandFlags::from_long_names(names.iter().map(String::as_str));
        assert_eq!(flags.as_str().len(), 30 * 42 + 29, "30 flags of 40 chars");
        let resource = Resource::current(&Uuid::new_v4().to_string());
        let mut events: Vec<Event> = (0..50).map(|_| command(flags.clone())).collect();
        assert!(
            encode(&resource, &events).to_string().len() > MAX_BODY_BYTES,
            "one request for all 50 would be over the cap"
        );

        let (mut sent, mut requests) = (0, 0);
        loop {
            let batch = take_batch(&resource, &mut events);
            if batch.is_empty() {
                break;
            }
            let body = serde_json::to_vec(&encode(&resource, &batch)).unwrap();
            assert!(body.len() <= MAX_BODY_BYTES, "{} bytes", body.len());
            sent += batch.len();
            requests += 1;
        }
        assert_eq!(sent, 50);
        assert_eq!(requests, 2);
        assert!(events.is_empty());
    }

    #[test]
    fn small_events_still_batch_by_count() {
        let resource = Resource::current(&Uuid::new_v4().to_string());
        let mut events: Vec<Event> = (0..120).map(|_| command(CommandFlags::default())).collect();
        assert_eq!(take_batch(&resource, &mut events).len(), MAX_BATCH);
        assert_eq!(events.len(), 70);
    }

    #[test]
    fn an_event_too_large_for_any_request_is_dropped_alone() {
        let resource = Resource::current(&Uuid::new_v4().to_string());
        let small = command(CommandFlags::default());
        // Not buildable from a command line or a spool line.
        let huge = command(CommandFlags::unchecked("a".repeat(MAX_BODY_BYTES)));
        let mut events = vec![huge, small.clone(), small];
        assert_eq!(take_batch(&resource, &mut events).len(), 2);
        assert!(events.is_empty());
    }
}
