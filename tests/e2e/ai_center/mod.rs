use std::sync::OnceLock;
use std::time::Duration;

use crate::harness;
use serde_json::{json, Value};

#[test]
#[ignore]
fn ai_center_applications_list() {
    if harness::require_creds("ai_center_applications_list").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["ai-center", "applications", "list", "-o", "json"]);
    harness::assert_array_of_objects_with_keys(&v, &["id", "application", "subsystem"]);

    // paging + evaluation-type filter flags are accepted and honoured
    let page = harness::run_ok_json(&[
        "ai-center",
        "applications",
        "list",
        "--page-size",
        "1",
        "--page-offset",
        "0",
        "-o",
        "json",
    ]);
    assert!(
        harness::assert_array(&page).len() <= 1,
        "page-size 1 should return at most one app: {page}"
    );
    let filtered = harness::run_ok_json(&[
        "ai-center",
        "applications",
        "list",
        "--evaluation-type",
        "PII",
        "-o",
        "json",
    ]);
    harness::assert_array(&filtered);
    harness::run_ok_nonempty(&["ai-center", "applications", "list"]);
    harness::run_ok_nonempty(&["ai-center", "applications", "list", "-o", "toon"]);
}

#[test]
#[ignore]
fn ai_center_applications_get() {
    if harness::require_creds("ai_center_applications_get").is_none() {
        return;
    }
    let Some(app) = discover_application() else {
        eprintln!("[e2e] skipping ai_center_applications_get: no AI applications on test team");
        return;
    };
    let v = harness::run_ok_json(&["ai-center", "applications", "get", &app.id, "-o", "json"]);
    harness::assert_get_response(&v, &["id", "application", "subsystem"]);
    harness::run_ok_nonempty(&["ai-center", "applications", "get", &app.id]);
    harness::run_fail(&[
        "ai-center",
        "applications",
        "get",
        "00000000-0000-0000-0000-000000000000",
        "-o",
        "json",
    ]);
}

#[test]
#[ignore]
fn ai_center_evaluations_list() {
    if harness::require_creds("ai_center_evaluations_list").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["ai-center", "evaluations", "list", "-o", "json"]);
    harness::assert_array(&v);
}

#[test]
#[ignore]
fn ai_center_coverage() {
    if harness::require_creds("ai_center_coverage").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["ai-center", "coverage", "-o", "json"]);
    harness::assert_object_with_keys(&v, &["counts"]);
    harness::run_ok_nonempty(&["ai-center", "coverage"]);
}

#[test]
#[ignore]
fn ai_center_custom_evaluations_list() {
    if harness::require_creds("ai_center_custom_evaluations_list").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["ai-center", "custom-evaluations", "list", "-o", "json"]);
    harness::assert_array(&v);
}

// ── Write lifecycles (writable e2e team) ─────────────────────────────────────

/// The API allows one evaluation per (application, subsystem, type, target):
/// a parallel run holding the same slot gets a 409. The test walks this pool
/// of slots until a create succeeds, so overlapping runs do not collide.
const EVAL_TYPES: &[(&str, &str)] = &[
    ("pii", r#"{ "categories": ["EMAIL_ADDRESS"] }"#),
    ("toxicity", "{}"),
    ("sexism", "{}"),
    ("promptInjection", "{}"),
];
const EVAL_TARGETS: &[&str] = &["PROMPT", "RESPONSE"];

#[test]
#[ignore]
fn ai_center_evaluations_lifecycle() {
    if harness::require_write_creds("ai_center_evaluations_lifecycle").is_none() {
        return;
    }
    let apps = discover_applications();
    if apps.is_empty() {
        eprintln!(
            "[e2e] skipping ai_center_evaluations_lifecycle: no AI applications on test team"
        );
        return;
    }

    // create: first free slot in the pool; `isEnabled` stays false so the
    // live otel-demo traffic is never evaluated (no LLM cost).
    let mut created: Option<(Value, App, &str, &str)> = None;
    'slots: for app in &apps {
        for (eval_type, config) in EVAL_TYPES {
            for target in EVAL_TARGETS {
                let config: Value = serde_json::from_str(config).unwrap();
                let body = json!({
                    "application": app.application,
                    "subsystem": app.subsystem,
                    "target": target,
                    "isEnabled": false,
                    "threshold": 0.5,
                    "config": { *eval_type: config },
                });
                let file = harness::JsonFile::new("aieval", &body);
                let out = harness::run_raw(&[
                    "ai-center",
                    "evaluations",
                    "create",
                    "--from-file",
                    file.path(),
                    "-o",
                    "json",
                    "--yes",
                ]);
                if out.status.success() {
                    let v = harness::parse_json(&out.stdout)
                        .unwrap_or_else(|| panic!("create should print JSON"));
                    created = Some((v, app.clone(), eval_type, target));
                    break 'slots;
                }
                let stderr = String::from_utf8_lossy(&out.stderr);
                assert!(
                    stderr.contains("(409)"),
                    "evaluation create failed for a reason other than a taken slot: {stderr}"
                );
            }
        }
    }
    let Some((resp, app, eval_type, target)) = created else {
        eprintln!("[e2e] skipping ai_center_evaluations_lifecycle: every evaluation slot is taken");
        return;
    };
    let id = harness::id_from_response(&resp)
        .unwrap_or_else(|| panic!("evaluation create returned no id: {resp}"));
    let mut cleanup = harness::Cleanup::new(&["ai-center", "evaluations", "delete", &id]);

    // get
    let got = evaluation_of(&harness::run_ok_json(&[
        "ai-center",
        "evaluations",
        "get",
        &id,
        "-o",
        "json",
    ]));
    assert_eq!(got["id"], id);
    assert_eq!(got["application"], app.application);
    assert_eq!(got["subsystem"], app.subsystem);
    assert_eq!(got["target"], target);
    assert_eq!(
        got["isEnabled"], false,
        "evaluation must stay disabled: {got}"
    );
    assert!(got["config"].get(eval_type).is_some(), "config type: {got}");

    // list (per app/subsystem and per type) contains it
    let list = harness::run_ok_json(&[
        "ai-center",
        "evaluations",
        "list",
        "--application",
        &app.application,
        "--subsystem",
        &app.subsystem,
        "-o",
        "json",
    ]);
    assert!(
        harness::find_by_name(&list, "id", &id).is_some(),
        "evaluations list for the app should contain {id}: {list}"
    );
    let list = harness::run_ok_json(&[
        "ai-center",
        "evaluations",
        "list",
        "--evaluation-type",
        &eval_type_enum(eval_type),
        "--page-size",
        "100",
        "--page-offset",
        "0",
        "-o",
        "json",
    ]);
    assert!(
        harness::find_by_name(&list, "id", &id).is_some(),
        "evaluations list by type should contain {id}: {list}"
    );

    // update: raise the threshold
    let upd = harness::JsonFile::new("aieval-upd", &json!({ "threshold": 0.7 }));
    harness::run_write_ok(&[
        "ai-center",
        "evaluations",
        "update",
        &id,
        "--from-file",
        upd.path(),
        "-o",
        "json",
    ]);
    let got = evaluation_of(&harness::run_ok_json(&[
        "ai-center",
        "evaluations",
        "get",
        &id,
        "-o",
        "json",
    ]));
    assert_eq!(
        got["threshold"],
        json!(0.7),
        "update should change the threshold: {got}"
    );
    assert_eq!(got["isEnabled"], false);

    // text + toon renderers on a known resource
    harness::run_ok_nonempty(&["ai-center", "evaluations", "get", &id]);
    harness::run_ok_nonempty(&["ai-center", "evaluations", "get", &id, "-o", "toon"]);
    harness::run_ok_nonempty(&["ai-center", "evaluations", "list"]);

    // delete, then get must fail
    harness::run_write_ok(&["ai-center", "evaluations", "delete", &id]);
    cleanup.disarm();
    harness::run_fail(&["ai-center", "evaluations", "get", &id, "-o", "json"]);
    harness::run_fail(&["ai-center", "evaluations", "delete", &id, "--yes"]);
}

#[test]
#[ignore]
fn ai_center_custom_evaluations_lifecycle() {
    if harness::require_write_creds("ai_center_custom_evaluations_lifecycle").is_none() {
        return;
    }
    let name = harness::unique_name("policy");

    // create
    let file = harness::JsonFile::new(
        "policy",
        &json!({
            "name": name,
            "description": "cx e2e policy",
            "instructions": "Flag any message that mentions the word probe.",
            "policyType": "SECURITY",
        }),
    );
    let created = harness::run_write_json(&[
        "ai-center",
        "custom-evaluations",
        "create",
        "--from-file",
        file.path(),
        "-o",
        "json",
    ]);
    let id = harness::id_from_response(&created)
        .unwrap_or_else(|| panic!("custom evaluation create returned no id: {created}"));
    let mut cleanup = CustomEvaluationCleanup::new(&id);

    // list contains it by name
    let list = harness::run_ok_json(&["ai-center", "custom-evaluations", "list", "-o", "json"]);
    let item = harness::find_by_name(&list, "name", &name)
        .unwrap_or_else(|| panic!("custom-evaluations list should contain {name}: {list}"));
    assert_eq!(item["id"], id);

    // update: change the description
    let upd = harness::JsonFile::new("policy-upd", &json!({ "description": "cx e2e policy v2" }));
    harness::run_write_ok(&[
        "ai-center",
        "custom-evaluations",
        "update",
        &id,
        "--from-file",
        upd.path(),
        "-o",
        "json",
    ]);
    let list = harness::run_ok_json(&["ai-center", "custom-evaluations", "list", "-o", "json"]);
    let item = harness::find_by_name(&list, "name", &name).expect("still listed");
    assert_eq!(
        item["description"], "cx e2e policy v2",
        "update should change the description"
    );

    // add <-> remove on an application
    if let Some(app) = discover_application() {
        harness::run_write_ok(&[
            "ai-center",
            "custom-evaluations",
            "add",
            &id,
            &app.id,
            "-o",
            "json",
        ]);
        let linked = harness::eventually(10, Duration::from_secs(2), || {
            let v = harness::run_ok_json(&[
                "ai-center",
                "custom-evaluations",
                "list-for-application",
                &app.id,
                "-o",
                "json",
            ]);
            harness::find_by_name(&v, "id", &id).cloned()
        });
        assert!(
            linked.is_some(),
            "list-for-application should contain {id} after add"
        );

        harness::run_write_ok(&[
            "ai-center",
            "custom-evaluations",
            "remove",
            &id,
            &app.id,
            "-o",
            "json",
        ]);
        let unlinked = harness::eventually(10, Duration::from_secs(2), || {
            let v = harness::run_ok_json(&[
                "ai-center",
                "custom-evaluations",
                "list-for-application",
                &app.id,
                "-o",
                "json",
            ]);
            harness::find_by_name(&v, "id", &id).is_none().then_some(())
        });
        assert!(
            unlinked.is_some(),
            "list-for-application should not contain {id} after remove"
        );
        harness::run_ok_nonempty(&[
            "ai-center",
            "custom-evaluations",
            "list-for-application",
            &app.id,
            "-o",
            "toon",
        ]);
    } else {
        eprintln!("[e2e] not covered: custom-evaluations add/remove/list-for-application — no AI application on team");
    }

    // text + toon renderers
    harness::run_ok_nonempty(&["ai-center", "custom-evaluations", "list"]);
    harness::run_ok_nonempty(&["ai-center", "custom-evaluations", "list", "-o", "toon"]);

    // negative: update/add on an unknown policy
    harness::run_fail(&[
        "ai-center",
        "custom-evaluations",
        "update",
        "00000000-0000-0000-0000-000000000000",
        "--from-file",
        upd.path(),
        "--yes",
    ]);

    // The CLI has no `custom-evaluations delete` by design (see the AI
    // Center skill), so the policy is removed through the API; the list
    // must then no longer show it.
    delete_custom_evaluation(&id).expect("delete the custom evaluation through the API");
    cleanup.disarm();
    let list = harness::run_ok_json(&["ai-center", "custom-evaluations", "list", "-o", "json"]);
    assert!(
        harness::find_by_name(&list, "name", &name).is_none(),
        "deleted policy {name} should disappear from the list: {list}"
    );
}

/// `DELETE /mgmt/openapi/5/ai/custom-evaluations/v3/{id}`: the API has the
/// endpoint, the CLI deliberately does not expose it.
pub(crate) fn delete_custom_evaluation(id: &str) -> Result<(), String> {
    let client = harness::api_client();
    let path = format!("/mgmt/openapi/5/ai/custom-evaluations/v3/{id}");
    harness::block_on(client.delete::<serde_json::Value>(&path))
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Drop guard that deletes a custom evaluation through the API, so a failed
/// assertion never leaves one behind.
struct CustomEvaluationCleanup {
    id: String,
    armed: bool,
}

impl CustomEvaluationCleanup {
    fn new(id: &str) -> Self {
        Self {
            id: id.to_string(),
            armed: true,
        }
    }
    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for CustomEvaluationCleanup {
    fn drop(&mut self) {
        if self.armed {
            if let Err(e) = delete_custom_evaluation(&self.id) {
                eprintln!(
                    "[e2e] cleanup: could not delete custom evaluation {}: {e}",
                    self.id
                );
            }
        }
    }
}

/// `model-pricing set` is team-wide: a safe idempotent round-trip writes the
/// current `prices` map back and asserts nothing changed.
#[test]
#[ignore]
fn ai_center_model_pricing_roundtrip() {
    if harness::require_write_creds("ai_center_model_pricing_roundtrip").is_none() {
        return;
    }
    let before = harness::run_ok_json(&["ai-center", "model-pricing", "get", "-o", "json"]);
    let prices = before
        .pointer("/pricing/prices")
        .cloned()
        .unwrap_or_else(|| panic!("model-pricing get should return pricing.prices: {before}"));
    assert!(prices.is_object(), "prices should be a map: {before}");

    let file = harness::JsonFile::new("prices", &prices);
    harness::run_write_ok(&[
        "ai-center",
        "model-pricing",
        "set",
        "--from-file",
        file.path(),
        "-o",
        "json",
    ]);

    let after = harness::run_ok_json(&["ai-center", "model-pricing", "get", "-o", "json"]);
    assert_eq!(
        after.pointer("/pricing/prices"),
        Some(&prices),
        "writing the same prices back must not change them"
    );
    harness::run_ok_nonempty(&["ai-center", "model-pricing", "get"]);
    harness::run_ok_nonempty(&["ai-center", "model-pricing", "get", "-o", "toon"]);
}

// ── helpers ──────────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct App {
    id: String,
    application: String,
    subsystem: String,
}

fn evaluation_of(v: &Value) -> Value {
    v.get("aiEvaluation").cloned().unwrap_or_else(|| v.clone())
}

/// `config` oneof key -> the `--evaluation-type` enum the list filter takes.
fn eval_type_enum(config_key: &str) -> String {
    let mut out = String::new();
    for c in config_key.chars() {
        if c.is_ascii_uppercase() {
            out.push('_');
        }
        out.push(c.to_ascii_uppercase());
    }
    out
}

fn discover_applications() -> Vec<App> {
    static CACHE: OnceLock<Vec<App>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            let stdout = harness::run_ok(&["ai-center", "applications", "list", "-o", "json"]);
            let Some(v) = harness::parse_json(&stdout) else {
                return Vec::new();
            };
            v.as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|item| {
                            Some(App {
                                id: item.get("id")?.as_str()?.to_string(),
                                application: item.get("application")?.as_str()?.to_string(),
                                subsystem: item.get("subsystem")?.as_str()?.to_string(),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default()
        })
        .clone()
}

/// Prefer the seeded `otel-demo` application; fall back to any.
fn discover_application() -> Option<App> {
    let apps = discover_applications();
    apps.iter()
        .find(|a| a.application == "otel-demo")
        .or(apps.first())
        .cloned()
}
