//! E2E coverage for `cx webhooks` (outgoing webhooks).
//!
//! The lifecycle test creates a GENERIC webhook pointing at IANA's reserved
//! `example.com` host. The API resolves the host when the webhook is saved
//! (`example.invalid` is rejected with "Couldn't resolve given host"), and
//! `webhooks test` really delivers once: example.com answers 405, which the
//! API reports as a `failure` payload with exit 0. Nobody is notified.

use std::sync::OnceLock;
use std::time::Duration;

use crate::harness;
use serde_json::{json, Value};

#[test]
#[ignore]
fn webhooks_list() {
    if harness::require_creds("webhooks_list").is_none() {
        return;
    }
    // The writable e2e team starts with no webhooks; the lifecycle test
    // creates its own, so only the shape is asserted here.
    let v = harness::run_ok_json(&["webhooks", "list", "-o", "json"]);
    harness::assert_array_of_objects_with_keys(&v, &["id", "name", "type"]);
}

#[test]
#[ignore]
fn webhooks_get() {
    if harness::require_creds("webhooks_get").is_none() {
        return;
    }
    let id = match discover_webhook_id() {
        Some(id) => id,
        None => {
            eprintln!("[e2e] skipping webhooks_get: no webhooks available on test team");
            return;
        }
    };
    let v = harness::run_ok_json(&["webhooks", "get", &id, "-o", "json"]);
    harness::assert_get_response(&v, &["id", "name"]);
}

#[test]
#[ignore]
fn webhooks_types() {
    if harness::require_creds("webhooks_types").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["webhooks", "types", "-o", "json"]);
    harness::assert_object_with_keys(&v, &["webhooks"]);
    harness::assert_nonempty_array_of_objects_with_keys(&v["webhooks"], &["type", "label"]);
    assert!(
        v["webhooks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["type"] == "GENERIC"),
        "webhook types should include GENERIC: {v}"
    );
    harness::run_ok_nonempty(&["webhooks", "types"]);
}

#[test]
#[ignore]
fn webhooks_get_nonexistent_fails() {
    if harness::require_creds("webhooks_get_nonexistent_fails").is_none() {
        return;
    }
    harness::run_fail(&[
        "webhooks",
        "get",
        "00000000-0000-0000-0000-000000000000",
        "-o",
        "json",
    ]);
}

fn discover_webhook_id() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            harness::require_creds("webhooks_discover")?;
            let stdout = harness::run_ok(&["webhooks", "list", "-o", "json"]);
            let v = harness::parse_json(&stdout)?;
            v.as_array()?
                .first()
                .and_then(|item| item.get("id"))
                .map(|x| match x {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string().trim_matches('"').to_string(),
                })
        })
        .clone()
}

/// A random-looking RFC 4122 v4 shaped id. The generic webhook config needs
/// a `uuid` on create and update; nothing checks it beyond the format, so a
/// time/pid based value is enough and avoids a uuid dev-dependency.
fn pseudo_uuid() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seed = nanos ^ ((std::process::id() as u128) << 64);
    format!(
        "{:08x}-{:04x}-4{:03x}-8{:03x}-{:012x}",
        (seed & 0xffff_ffff) as u32,
        ((seed >> 32) & 0xffff) as u16,
        ((seed >> 48) & 0xfff) as u16,
        ((seed >> 60) & 0xfff) as u16,
        ((seed >> 72) & 0xffff_ffff_ffff) as u64
    )
}

/// Create/update `data` for a GENERIC webhook named `name`.
fn generic_webhook_data(name: &str) -> Value {
    json!({
        "name": name,
        "type": "GENERIC",
        "url": format!("https://example.com/cx-e2e/{name}"),
        "genericWebhook": {
            "method": "POST",
            "headers": { "content-type": "application/json" },
            "payload": "{\"text\":\"cx e2e\"}",
            "uuid": pseudo_uuid()
        }
    })
}

// ── Webhook lifecycle (writable e2e team) ───────────────────────────────────

#[test]
#[ignore]
fn webhooks_lifecycle() {
    if harness::require_write_creds("webhooks_lifecycle").is_none() {
        return;
    }
    let name = harness::unique_name("webhook");

    // create
    let file = harness::JsonFile::new("webhook", &json!({ "data": generic_webhook_data(&name) }));
    let created = harness::run_write_json(&[
        "webhooks",
        "create",
        "--from-file",
        file.path(),
        "-o",
        "json",
    ]);
    let id = harness::id_from_response(&created)
        .unwrap_or_else(|| panic!("webhook create returned no id: {created}"));
    let mut cleanup = harness::Cleanup::new(&["webhooks", "delete", &id]);

    // get
    let got = harness::run_ok_json(&["webhooks", "get", &id, "-o", "json"]);
    harness::assert_get_response(&got, &["id", "name", "type"]);
    assert_eq!(got["webhook"]["name"], name, "get should return our name");
    assert_eq!(got["webhook"]["type"], "GENERIC");

    // list contains it
    let found = harness::eventually(10, Duration::from_secs(2), || {
        let list = harness::run_ok_json(&["webhooks", "list", "-o", "json"]);
        harness::find_by_name(&list, "name", &name)
            .is_some()
            .then_some(())
    });
    assert!(found.is_some(), "webhook list should contain {name}");

    // update <id>: rename; the id comes from the command line, the file
    // holds only the new data
    let renamed = format!("{name}-renamed");
    let upd = harness::JsonFile::new(
        "webhook-upd",
        &json!({ "data": generic_webhook_data(&renamed) }),
    );
    harness::run_write_ok(&[
        "webhooks",
        "update",
        &id,
        "--from-file",
        upd.path(),
        "-o",
        "json",
    ]);
    let got = harness::run_ok_json(&["webhooks", "get", &id, "-o", "json"]);
    assert_eq!(got["webhook"]["name"], renamed, "update should rename");

    // test <id>: one real delivery to example.com. The API answers 200 with
    // a success or failure verdict either way.
    let verdict = harness::run_write_json(&["webhooks", "test", &id, "-o", "json"]);
    let obj = verdict
        .as_object()
        .unwrap_or_else(|| panic!("expected JSON object, got: {verdict}"));
    assert!(
        obj.contains_key("success") || obj.contains_key("failure"),
        "webhook test should report a verdict: {verdict}"
    );

    // text + toon renderers on a known resource
    harness::run_ok_nonempty(&["webhooks", "get", &id]);
    harness::run_ok_nonempty(&["webhooks", "get", &id, "-o", "toon"]);
    harness::run_ok_nonempty(&["webhooks", "list"]);

    // delete, then get must fail
    harness::run_write_ok(&["webhooks", "delete", &id]);
    cleanup.disarm();
    harness::run_fail(&["webhooks", "get", &id, "-o", "json"]);
    harness::run_fail(&["webhooks", "delete", &id, "--yes"]);
}
