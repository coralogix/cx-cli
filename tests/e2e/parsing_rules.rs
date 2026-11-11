use std::sync::OnceLock;

use crate::harness;
use serde_json::{json, Value};

#[test]
#[ignore]
fn parsing_rules_list() {
    if harness::require_creds("parsing_rules_list").is_none() {
        return;
    }
    // The test team ships with a seed "Severity Rules" group.
    let v = harness::run_ok_json(&["parsing-rules", "list", "-o", "json"]);
    harness::assert_nonempty_array_of_objects_with_keys(&v, &["id", "name"]);
}

#[test]
#[ignore]
fn parsing_rules_get() {
    if harness::require_creds("parsing_rules_get").is_none() {
        return;
    }
    let id = match discover_parsing_rule_id() {
        Some(id) => id,
        None => {
            eprintln!("[e2e] skipping parsing_rules_get: no parsing rules available on test team");
            return;
        }
    };
    let v = harness::run_ok_json(&["parsing-rules", "get", &id, "-o", "json"]);
    harness::assert_get_response(&v, &["id", "name"]);
}

#[test]
#[ignore]
fn parsing_rules_usage_limits() {
    if harness::require_creds("parsing_rules_usage_limits").is_none() {
        return;
    }
    let v = harness::run_ok_json(&["parsing-rules", "usage-limits", "-o", "json"]);
    harness::assert_object_with_keys(&v, &["limits", "usage"]);
    harness::assert_object_with_keys(&v["limits"], &["groups", "rules"]);
    harness::run_ok_nonempty(&["parsing-rules", "usage-limits"]);
    harness::run_ok_nonempty(&["parsing-rules", "usage-limits", "-o", "toon"]);
}

fn discover_parsing_rule_id() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            harness::require_creds("parsing_rules_discover")?;
            let stdout = harness::run_ok(&["parsing-rules", "list", "-o", "json"]);
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

// ── Write lifecycle (writable e2e team) ──────────────────────────────────────

/// A rule group that can never touch live data: it is disabled and its
/// matcher is scoped to a subsystem that does not exist (our unique name).
/// `order: 0` places it last so it never shifts the seed groups.
fn rule_group_body(name: &str, description: &str) -> Value {
    json!({
        "name": name,
        "description": description,
        "creator": "cx-e2e",
        "enabled": false,
        "hidden": false,
        "order": 0,
        "ruleMatchers": [ { "subsystemName": { "value": name } } ],
        "ruleSubgroups": [
            {
                "order": 1,
                "enabled": true,
                "rules": [
                    {
                        "name": "extract",
                        "description": "e2e extract rule",
                        "sourceField": "text",
                        "order": 1,
                        "enabled": true,
                        "parameters": {
                            "extractParameters": { "rule": "(?P<cx_e2e_field>cx-e2e-never-matches)" }
                        }
                    }
                ]
            }
        ]
    })
}

/// `get` wraps the group as `{"ruleGroup": {...}}`.
fn rule_group_of(get_response: &Value) -> &Value {
    get_response.get("ruleGroup").unwrap_or(get_response)
}

fn create_rule_group(name: &str, description: &str) -> String {
    let file = harness::JsonFile::new("rg", &rule_group_body(name, description));
    let created = harness::run_write_json(&[
        "parsing-rules",
        "create",
        "--from-file",
        file.path(),
        "-o",
        "json",
    ]);
    assert_eq!(created["name"], name, "create should echo our name");
    harness::id_from_response(&created)
        .unwrap_or_else(|| panic!("rule group create returned no id: {created}"))
}

#[test]
#[ignore]
fn parsing_rules_lifecycle() {
    if harness::require_write_creds("parsing_rules_lifecycle").is_none() {
        return;
    }
    let name = harness::unique_name("rg");

    // create
    let id = create_rule_group(&name, "e2e rule group");
    let mut cleanup = harness::Cleanup::new(&["parsing-rules", "delete", &id]);

    // get
    let got = harness::run_ok_json(&["parsing-rules", "get", &id, "-o", "json"]);
    harness::assert_get_response(&got, &["id", "name"]);
    let group = rule_group_of(&got);
    assert_eq!(group["name"], name);
    assert_eq!(group["enabled"], json!(false), "group must stay disabled");
    // The API stores matcher values lowercased; the group name keeps its case.
    let matcher = group
        .pointer("/ruleMatchers/0/subsystemName/value")
        .and_then(Value::as_str)
        .unwrap_or("");
    assert!(
        matcher.eq_ignore_ascii_case(&name),
        "matcher must be scoped to our subsystem: {got}"
    );

    // list contains it, with the nested rule counted
    let list = harness::run_ok_json(&["parsing-rules", "list", "-o", "json"]);
    let ours = harness::find_by_name(&list, "name", &name)
        .unwrap_or_else(|| panic!("rule group list should contain {name}"));
    assert_eq!(
        ours["rules_count"],
        json!(1),
        "list should count subgroup rules"
    );
    assert_eq!(ours["enabled"], json!(false));

    // update: replace with a new description, still disabled
    let upd = harness::JsonFile::new("rg-upd", &rule_group_body(&name, "e2e rule group updated"));
    harness::run_write_ok(&[
        "parsing-rules",
        "update",
        "--from-file",
        upd.path(),
        &id,
        "-o",
        "json",
    ]);
    let got = harness::run_ok_json(&["parsing-rules", "get", &id, "-o", "json"]);
    let group = rule_group_of(&got);
    assert_eq!(group["description"], json!("e2e rule group updated"));
    assert_eq!(group["enabled"], json!(false));

    // text + toon renderers on a known resource
    harness::run_ok_nonempty(&["parsing-rules", "get", &id]);
    harness::run_ok_nonempty(&["parsing-rules", "get", &id, "-o", "toon"]);
    harness::run_ok_nonempty(&["parsing-rules", "list"]);
    harness::run_ok_nonempty(&["parsing-rules", "list", "-o", "toon"]);

    // delete, then get must fail
    harness::run_write_ok(&["parsing-rules", "delete", &id]);
    cleanup.disarm();
    harness::run_fail(&["parsing-rules", "get", &id, "-o", "json"]);
}

#[test]
#[ignore]
fn parsing_rules_bulk_delete() {
    if harness::require_write_creds("parsing_rules_bulk_delete").is_none() {
        return;
    }
    let name_a = harness::unique_name("rg");
    let name_b = harness::unique_name("rg");
    let id_a = create_rule_group(&name_a, "e2e bulk a");
    let mut cleanup_a = harness::Cleanup::new(&["parsing-rules", "delete", &id_a]);
    let id_b = create_rule_group(&name_b, "e2e bulk b");
    let mut cleanup_b = harness::Cleanup::new(&["parsing-rules", "delete", &id_b]);

    harness::run_write_ok(&["parsing-rules", "bulk-delete", "--ids", &id_a, &id_b]);
    cleanup_a.disarm();
    cleanup_b.disarm();

    harness::run_fail(&["parsing-rules", "get", &id_a, "-o", "json"]);
    harness::run_fail(&["parsing-rules", "get", &id_b, "-o", "json"]);
    let list = harness::run_ok_json(&["parsing-rules", "list", "-o", "json"]);
    assert!(harness::find_by_name(&list, "name", &name_a).is_none());
    assert!(harness::find_by_name(&list, "name", &name_b).is_none());
}

#[test]
#[ignore]
fn parsing_rules_get_and_delete_missing_fail() {
    if harness::require_write_creds("parsing_rules_get_and_delete_missing_fail").is_none() {
        return;
    }
    let bogus = "00000000-0000-4000-8000-000000000000";
    harness::run_fail(&["parsing-rules", "get", bogus, "-o", "json"]);
    harness::run_fail(&["parsing-rules", "delete", bogus, "--yes"]);
}
