use std::sync::OnceLock;

use crate::harness;
use serde_json::{json, Value};

/// A pull-based contextual-data type with only free-form fields (a name and
/// an RSS/Atom feed URL): no credentials and nothing to pay for. `get <key>`
/// lists its registered instances, so the lifecycle can assert on ids.
const LIFECYCLE_KEY: &str = "StatusPage-Tracker";

#[test]
#[ignore]
fn contextual_data_list() {
    if harness::require_creds("contextual_data_list").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["integrations", "contextual-data", "list", "-o", "json"]);
    harness::assert_nonempty_array_of_objects_with_keys(&v, &["id", "name"]);
}

#[test]
#[ignore]
fn contextual_data_get() {
    if harness::require_creds("contextual_data_get").is_none() {
        return;
    }
    let id = match discover_contextual_data_id() {
        Some(id) => id,
        None => {
            eprintln!(
                "[e2e] skipping contextual_data_get: no contextual data available on test team"
            );
            return;
        }
    };
    let v = harness::run_ok_json(&["integrations", "contextual-data", "get", &id, "-o", "json"]);
    harness::assert_get_response(&v, &["id", "name", "instances"]);
    harness::run_fail(&[
        "integrations",
        "contextual-data",
        "get",
        "cx-e2e-no-such-key",
        "-o",
        "json",
    ]);
}

#[test]
#[ignore]
fn contextual_data_definition() {
    if harness::require_creds("contextual_data_definition").is_none() {
        return;
    }
    let v = harness::run_ok_json(&[
        "integrations",
        "contextual-data",
        "definition",
        LIFECYCLE_KEY,
        "-o",
        "json",
    ]);
    assert_eq!(
        v.pointer("/integrationDefinition/key")
            .and_then(Value::as_str),
        Some(LIFECYCLE_KEY),
        "definition should echo the key: {v}"
    );
    assert!(
        definition_version(&v).is_some(),
        "definition should list a revision: {v}"
    );
    harness::run_ok_nonempty(&[
        "integrations",
        "contextual-data",
        "definition",
        LIFECYCLE_KEY,
    ]);
    harness::run_fail(&[
        "integrations",
        "contextual-data",
        "definition",
        "cx-e2e-no-such-key",
        "-o",
        "json",
    ]);
}

// ── Write lifecycle (writable e2e team) ──────────────────────────────────────

#[test]
#[ignore]
fn contextual_data_lifecycle() {
    if harness::require_write_creds("contextual_data_lifecycle").is_none() {
        return;
    }
    let definition = harness::run_ok_json(&[
        "integrations",
        "contextual-data",
        "definition",
        LIFECYCLE_KEY,
        "-o",
        "json",
    ]);
    let version = definition_version(&definition)
        .unwrap_or_else(|| panic!("no revision in definition: {definition}"));
    let name = harness::unique_name("ctxdata");
    let metadata = |integration_name: &str| {
        json!({
            "integrationKey": LIFECYCLE_KEY,
            "version": version,
            "integrationParameters": { "parameters": [
                { "key": "IntegrationName", "stringValue": integration_name },
                { "key": "RSSFeedUrl", "stringValue": "https://www.githubstatus.com/history.atom" },
            ] }
        })
    };

    // create (API shape with the `metadata` envelope)
    let file = harness::JsonFile::new("ctxdata", &json!({ "metadata": metadata(&name) }));
    let created = harness::run_write_json(&[
        "integrations",
        "contextual-data",
        "create",
        "--from-file",
        file.path(),
        "-o",
        "json",
    ]);
    let id = harness::id_from_response(&created)
        .unwrap_or_else(|| panic!("contextual-data create returned no id: {created}"));
    let mut cleanup = harness::Cleanup::new(&["integrations", "contextual-data", "delete", &id]);

    // get <key>: instances contain ours with our name
    let got = harness::run_ok_json(&[
        "integrations",
        "contextual-data",
        "get",
        LIFECYCLE_KEY,
        "-o",
        "json",
    ]);
    assert_eq!(got["id"], LIFECYCLE_KEY);
    let instance = instance_by_id(&got, &id)
        .unwrap_or_else(|| panic!("get {LIFECYCLE_KEY} should list instance {id}: {got}"));
    assert_eq!(
        instance_param(instance, "IntegrationName"),
        Some(name.as_str()),
        "instance should carry our name: {instance}"
    );

    // list: the type now has at least one instance
    let list = harness::run_ok_json(&["integrations", "contextual-data", "list", "-o", "json"]);
    let entry = harness::find_by_name(&list, "id", LIFECYCLE_KEY)
        .unwrap_or_else(|| panic!("list should contain {LIFECYCLE_KEY}"));
    assert!(
        entry["instances"].as_i64().unwrap_or(0) >= 1,
        "list should count our instance: {entry}"
    );

    // update / test: the API does not support either for this type (nor
    // for the push-based webhook types). A clean 400 proves the routes.
    let upd = harness::JsonFile::new(
        "ctxdata-upd",
        &json!({ "metadata": metadata(&format!("{name}-renamed")) }),
    );
    let stderr = harness::run_fail(&[
        "integrations",
        "contextual-data",
        "update",
        &id,
        "--from-file",
        upd.path(),
        "--yes",
        "-o",
        "json",
    ]);
    assert!(stderr.contains("(400)"), "update unsupported: {stderr}");
    let stderr = harness::run_fail(&["integrations", "contextual-data", "test", &id, "-o", "json"]);
    assert!(stderr.contains("(400)"), "test unsupported: {stderr}");
    eprintln!(
        "[e2e] not covered: contextual-data update/test (success path) — only credentialed \
         pull types (e.g. slack, Cloudflare) support them and they need real third-party keys"
    );

    // text + toon renderers on a known resource
    harness::run_ok_nonempty(&["integrations", "contextual-data", "get", LIFECYCLE_KEY]);
    harness::run_ok_nonempty(&[
        "integrations",
        "contextual-data",
        "get",
        LIFECYCLE_KEY,
        "-o",
        "toon",
    ]);

    // delete, then the instance is gone and a second delete is a 404
    harness::run_write_ok(&["integrations", "contextual-data", "delete", &id]);
    cleanup.disarm();
    let got = harness::run_ok_json(&[
        "integrations",
        "contextual-data",
        "get",
        LIFECYCLE_KEY,
        "-o",
        "json",
    ]);
    assert!(
        instance_by_id(&got, &id).is_none(),
        "deleted instance should disappear from get: {got}"
    );
    let stderr = harness::run_fail(&["integrations", "contextual-data", "delete", &id, "--yes"]);
    assert!(stderr.contains("(404)"), "second delete: {stderr}");
}

fn definition_version(definition: &Value) -> Option<String> {
    definition
        .pointer("/integrationDefinition/revisions/0/id")
        .and_then(Value::as_str)
        .map(String::from)
}

fn instance_by_id<'a>(got: &'a Value, id: &str) -> Option<&'a Value> {
    got.get("instances")
        .and_then(Value::as_array)?
        .iter()
        .find(|i| i.get("id").and_then(Value::as_str) == Some(id))
}

fn instance_param<'a>(instance: &'a Value, key: &str) -> Option<&'a str> {
    instance
        .get("parameters")
        .and_then(Value::as_array)?
        .iter()
        .find(|p| p.get("key").and_then(Value::as_str) == Some(key))?
        .get("stringValue")
        .and_then(Value::as_str)
}

fn discover_contextual_data_id() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            harness::require_creds("contextual_data_discover")?;
            let stdout =
                harness::run_ok(&["integrations", "contextual-data", "list", "-o", "json"]);
            let v = harness::parse_json(&stdout)?;
            v.as_array()?
                .iter()
                .find(|item| !harness::is_e2e_item(item))
                .and_then(|item| item.get("id"))
                .map(|x| match x {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string().trim_matches('"').to_string(),
                })
        })
        .clone()
}
