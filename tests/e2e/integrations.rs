use std::sync::OnceLock;

use crate::harness;
use serde_json::{json, Value};

/// A cheap "untracked" integration type: registering it only stores metadata
/// (no cloud resources, no managed collector). Its fields are a Send-Your-Data
/// API key reference plus free-form names.
const LIFECYCLE_KEY: &str = "K8s-Collector";

#[test]
#[ignore]
fn integrations_list() {
    if harness::require_creds("integrations_list").is_none() {
        return;
    }
    // The catalog is global, so it is never empty.
    let v = harness::run_ok_json(&["integrations", "list", "-o", "json"]);
    harness::assert_nonempty_array_of_objects_with_keys(&v, &["id", "name", "type", "version"]);
}

#[test]
#[ignore]
fn integrations_get() {
    if harness::require_creds("integrations_get").is_none() {
        return;
    }
    let id = match discover_integration_id() {
        Some(id) => id,
        None => {
            eprintln!("[e2e] skipping integrations_get: no integrations available on test team");
            return;
        }
    };
    let v = harness::run_ok_json(&["integrations", "get", &id, "-o", "json"]);
    // `get` returns the catalog entry plus deployments. Update and test read
    // the id from this path, so the assertion has to follow it.
    let got = v
        .pointer("/integrationDetail/integration/id")
        .and_then(|value| value.as_str());
    assert_eq!(
        got,
        Some(id.as_str()),
        "integration get id at integrationDetail.integration.id"
    );
}

#[test]
#[ignore]
fn integrations_definition() {
    if harness::require_creds("integrations_definition").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["integrations", "definition", LIFECYCLE_KEY, "-o", "json"]);
    assert_eq!(
        v.pointer("/integrationDefinition/key")
            .and_then(Value::as_str),
        Some(LIFECYCLE_KEY),
        "definition should echo the key: {v}"
    );
    assert!(
        !definition_versions(&v).is_empty(),
        "definition should list at least one revision: {v}"
    );
    harness::run_ok_nonempty(&["integrations", "definition", LIFECYCLE_KEY]);
    harness::run_ok_nonempty(&["integrations", "definition", LIFECYCLE_KEY, "-o", "toon"]);
    harness::run_fail(&[
        "integrations",
        "definition",
        "cx-e2e-no-such-key",
        "-o",
        "json",
    ]);
}

#[test]
#[ignore]
fn integrations_template_without_id_is_rejected_by_api() {
    if harness::require_creds("integrations_template_without_id_is_rejected_by_api").is_none() {
        return;
    }
    // `cx integrations template` has no way to pass a deployed instance id
    // yet, and the template route requires one. The API answers 400 (not a
    // 404 for a wrong path), which proves the route is correct. Untracked
    // integrations have no template anyway, so a full round-trip needs a
    // CloudFormation/ARM deployment with real cloud parameters.
    let stderr = harness::run_fail(&["integrations", "template", "-o", "json"]);
    assert!(
        stderr.contains("(400)"),
        "template without an id should be a 400 from the API: {stderr}"
    );
    eprintln!(
        "[e2e] not covered: integrations template (success path) — the command cannot pass \
         --integration-id and untracked integrations have no template"
    );
}

#[test]
#[ignore]
fn integrations_unknown_instance_paths_fail() {
    if harness::require_creds("integrations_unknown_instance_paths_fail").is_none() {
        return;
    }
    let bogus = "00000000-0000-0000-0000-000000000000";
    let stderr = harness::run_fail(&["integrations", "deployed", bogus, "-o", "json"]);
    assert!(stderr.contains("(404)"), "deployed unknown: {stderr}");
    if harness::require_write_creds("integrations_unknown_instance_paths_fail").is_some() {
        let stderr = harness::run_fail(&["integrations", "delete", bogus, "--yes"]);
        assert!(stderr.contains("(404)"), "delete unknown: {stderr}");
    }
}

// ── Write lifecycle (writable e2e team) ──────────────────────────────────────

#[test]
#[ignore]
fn integrations_lifecycle() {
    if harness::require_write_creds("integrations_lifecycle").is_none() {
        return;
    }
    let Some(api_key_id) = discover_send_data_key_id() else {
        eprintln!(
            "[e2e] skipping integrations_lifecycle: no existing API key to reference as the \
             collector's Send-Your-Data key"
        );
        return;
    };
    let definition =
        harness::run_ok_json(&["integrations", "definition", LIFECYCLE_KEY, "-o", "json"]);
    let version = definition_versions(&definition)
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("no revision in definition: {definition}"));

    let name = harness::unique_name("integ");
    let parameters = |collector_name: &str| {
        json!([
            { "key": "PrivateKey", "apiKey": { "id": api_key_id } },
            { "key": "CollectorName", "stringValue": collector_name },
            { "key": "ApplicationName", "stringValue": name },
            { "key": "SubsystemName", "stringValue": name },
        ])
    };

    // create (flat input shape; `create_request` wraps it in `metadata`)
    let body = json!({
        "integrationKey": LIFECYCLE_KEY,
        "version": version,
        "parameters": parameters(&name),
    });
    let file = harness::JsonFile::new("integ", &body);
    let created = harness::run_write_json(&[
        "integrations",
        "create",
        "--from-file",
        file.path(),
        "-o",
        "json",
    ]);
    let id = harness::id_from_response(&created)
        .unwrap_or_else(|| panic!("integration create returned no id: {created}"));
    assert_eq!(created["integrationKey"], LIFECYCLE_KEY);
    let mut cleanup = harness::Cleanup::new(&["integrations", "delete", &id]);

    // deployed: our instance with our parameters
    let deployed = harness::run_ok_json(&["integrations", "deployed", &id, "-o", "json"]);
    assert_eq!(deployed.pointer("/integration/id"), Some(&json!(id)));
    assert_eq!(
        deployed.pointer("/integration/definitionKey"),
        Some(&json!(LIFECYCLE_KEY))
    );
    assert_eq!(
        deployed_param(&deployed, "CollectorName"),
        Some(name.as_str()),
        "deployed should carry our CollectorName: {deployed}"
    );

    // get <key>: registered deployments contain ours
    let got = harness::run_ok_json(&["integrations", "get", LIFECYCLE_KEY, "-o", "json"]);
    assert!(
        registered_ids(&got).contains(&id),
        "get {LIFECYCLE_KEY} should list our deployment {id}: {got}"
    );

    // list contains the type
    let list = harness::run_ok_json(&["integrations", "list", "-o", "json"]);
    assert!(
        harness::find_by_name(&list, "id", LIFECYCLE_KEY).is_some(),
        "list should contain {LIFECYCLE_KEY}"
    );

    // update: rename the collector (flat shape)
    let renamed = format!("{name}-renamed");
    let upd = harness::JsonFile::new(
        "integ-upd",
        &json!({
            "integrationKey": LIFECYCLE_KEY,
            "version": version,
            "parameters": parameters(&renamed),
        }),
    );
    harness::run_write_ok(&[
        "integrations",
        "update",
        &id,
        "--from-file",
        upd.path(),
        "-o",
        "json",
    ]);
    let deployed = harness::run_ok_json(&["integrations", "deployed", &id, "-o", "json"]);
    assert_eq!(
        deployed_param(&deployed, "CollectorName"),
        Some(renamed.as_str()),
        "update should rename the collector: {deployed}"
    );

    // update again from the `get` JSON (the documented round-trip input)
    let got = harness::run_ok_json(&["integrations", "get", LIFECYCLE_KEY, "-o", "json"]);
    let from_get = harness::JsonFile::new("integ-get", &got);
    harness::run_write_ok(&[
        "integrations",
        "update",
        &id,
        "--from-file",
        from_get.path(),
        "-o",
        "json",
    ]);
    let deployed = harness::run_ok_json(&["integrations", "deployed", &id, "-o", "json"]);
    assert_eq!(
        deployed_param(&deployed, "CollectorName"),
        Some(renamed.as_str())
    );

    // test: untracked types are not testable; the API says so with a 400.
    let stderr = harness::run_fail(&[
        "integrations",
        "test",
        "--id",
        &id,
        "--from-file",
        upd.path(),
        "-o",
        "json",
        "--yes",
    ]);
    assert!(
        stderr.contains("(400)"),
        "test on an untracked integration should be a clean 400: {stderr}"
    );
    eprintln!(
        "[e2e] not covered: integrations test (success path) — only managed integrations with \
         real third-party credentials can be tested"
    );

    // text + toon renderers on a known resource
    harness::run_ok_nonempty(&["integrations", "deployed", &id]);
    harness::run_ok_nonempty(&["integrations", "deployed", &id, "-o", "toon"]);

    // delete, then deployed must fail and the registration is gone
    harness::run_write_ok(&["integrations", "delete", &id]);
    cleanup.disarm();
    harness::run_fail(&["integrations", "deployed", &id, "-o", "json"]);
    let got = harness::run_ok_json(&["integrations", "get", LIFECYCLE_KEY, "-o", "json"]);
    assert!(
        !registered_ids(&got).contains(&id),
        "deleted deployment should disappear from get: {got}"
    );
}

fn definition_versions(definition: &Value) -> Vec<String> {
    definition
        .pointer("/integrationDefinition/revisions")
        .and_then(Value::as_array)
        .map(|revs| {
            revs.iter()
                .filter_map(|r| r.get("id").and_then(Value::as_str))
                .map(String::from)
                .collect()
        })
        .unwrap_or_default()
}

fn deployed_param<'a>(deployed: &'a Value, key: &str) -> Option<&'a str> {
    deployed
        .pointer("/integration/parameters")
        .and_then(Value::as_array)?
        .iter()
        .find(|p| p.get("key").and_then(Value::as_str) == Some(key))?
        .get("stringValue")
        .and_then(Value::as_str)
}

fn registered_ids(got: &Value) -> Vec<String> {
    got.pointer("/integrationDetail/default/registered")
        .and_then(Value::as_array)
        .map(|regs| {
            regs.iter()
                .filter_map(|r| r.get("id").and_then(Value::as_str))
                .map(String::from)
                .collect()
        })
        .unwrap_or_default()
}

/// An existing, active API key on the team to reference as the collector's
/// key. The key is only referenced by id (never modified or deleted), so
/// sharing it across parallel runs is safe. Keys created by other e2e tests
/// (`cx-e2e-*`) are skipped because they may vanish mid-test.
fn discover_send_data_key_id() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            let stdout = harness::run_ok(&["iam", "api-keys", "list", "-o", "json"]);
            let v = harness::parse_json(&stdout)?;
            v.as_array()?
                .iter()
                .filter(|k| {
                    k.get("name")
                        .and_then(Value::as_str)
                        .map(|n| !n.starts_with(harness::E2E_NAME_PREFIX))
                        .unwrap_or(true)
                })
                .filter(|k| !matches!(k.get("active"), Some(Value::Bool(false))))
                .find_map(|k| k.get("id").and_then(Value::as_str))
                .map(String::from)
        })
        .clone()
}

fn discover_integration_id() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            harness::require_creds("integrations_discover")?;
            let stdout = harness::run_ok(&["integrations", "list", "-o", "json"]);
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
