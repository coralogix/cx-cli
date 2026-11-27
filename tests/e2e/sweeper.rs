//! Stale-resource sweeper for the writable e2e team.
//!
//! Every write test names its resources with [`harness::unique_name`] and
//! removes them from active use on exit (also on panic, through
//! [`harness::Cleanup`]). A
//! killed runner or a lost network can still leave resources behind, so CI
//! runs this sweeper before the suite. It lists each resource type, keeps
//! only names produced by `unique_name` in a *different* run that are older
//! than [`MAX_AGE`], and removes those. Resources from the current run and
//! anything not named `cx-e2e-...` are never touched.
//!
//! With `CX_E2E_SWEEP_MODE=run` it instead removes every `cx-e2e-*` resource
//! that carries the *current* run id, whatever its age. CI runs that mode in
//! a final `if: always()` step, so a cancelled job (whose drop guards never
//! ran) still cleans up after itself.

use std::collections::HashSet;
use std::time::Duration;

use serde_json::Value;

use crate::harness;

/// Resources older than this (per the stamp in their name) are considered
/// leftovers. Longer than any CI job timeout, so a concurrent run can never
/// have its live resources swept.
const MAX_AGE: Duration = Duration::from_secs(2 * 60 * 60);

/// One sweepable resource type: how to list it and how to remove one.
struct Sweepable {
    label: &'static str,
    list: &'static [&'static str],
    delete: &'static [&'static str],
}

const SWEEPABLES: &[Sweepable] = &[
    Sweepable {
        label: "views",
        list: &["views", "list"],
        delete: &["views", "delete"],
    },
    Sweepable {
        label: "view folders",
        list: &["views", "folders", "list"],
        delete: &["views", "folders", "delete"],
    },
    Sweepable {
        label: "dashboards",
        list: &["dashboards", "catalog"],
        delete: &["dashboards", "delete"],
    },
    Sweepable {
        label: "dashboard folders",
        list: &["dashboards", "folders", "list"],
        delete: &["dashboards", "folders", "delete"],
    },
    Sweepable {
        label: "alerts",
        list: &["alerts", "list"],
        delete: &["alerts", "delete"],
    },
    Sweepable {
        label: "suppression rules",
        list: &["alerts", "suppression-rules", "list"],
        delete: &["alerts", "suppression-rules", "delete"],
    },
    Sweepable {
        label: "slos",
        list: &["slos", "list"],
        delete: &["slos", "delete"],
    },
    Sweepable {
        label: "product analytics funnels",
        list: &["product-analytics", "funnels", "list"],
        delete: &["product-analytics", "funnels", "delete"],
    },
    Sweepable {
        label: "product analytics pathways",
        list: &["product-analytics", "pathways", "list"],
        delete: &["product-analytics", "pathways", "delete"],
    },
    Sweepable {
        label: "product analytics heatmaps",
        list: &["product-analytics", "heatmaps", "list"],
        delete: &["product-analytics", "heatmaps", "delete"],
    },
    Sweepable {
        label: "webhooks",
        list: &["webhooks", "list"],
        delete: &["webhooks", "delete"],
    },
    Sweepable {
        label: "actions",
        list: &["webhooks", "actions", "list"],
        delete: &["webhooks", "actions", "delete"],
    },
    // Routers reference connectors, so they are swept first; a referenced
    // connector cannot be deleted.
    Sweepable {
        label: "routers",
        list: &["notifications", "routers", "list"],
        delete: &["notifications", "routers", "delete"],
    },
    Sweepable {
        label: "connectors",
        list: &["notifications", "connectors", "list"],
        delete: &["notifications", "connectors", "delete"],
    },
    Sweepable {
        label: "presets",
        list: &["notifications", "presets", "list"],
        delete: &["notifications", "presets", "delete"],
    },
    Sweepable {
        label: "parsing rule groups",
        list: &["parsing-rules", "list"],
        delete: &["parsing-rules", "delete"],
    },
    Sweepable {
        label: "custom enrichments",
        list: &["enrichments", "custom", "list"],
        delete: &["enrichments", "custom", "delete"],
    },
    Sweepable {
        label: "e2m",
        list: &["e2m", "list"],
        delete: &["e2m", "delete"],
    },
    Sweepable {
        label: "recording rule groups",
        list: &["recording-rules", "list"],
        delete: &["recording-rules", "delete"],
    },
    Sweepable {
        label: "configuration groups",
        list: &["configuration-groups", "list"],
        delete: &["configuration-groups", "archive"],
    },
    Sweepable {
        label: "configuration overlays",
        list: &["configuration-overlays", "list"],
        delete: &["configuration-overlays", "archive"],
    },
    Sweepable {
        label: "tco policies",
        list: &["tco", "list"],
        delete: &["tco", "delete"],
    },
    Sweepable {
        label: "api keys",
        list: &["iam", "api-keys", "list"],
        delete: &["iam", "api-keys", "delete"],
    },
    Sweepable {
        label: "roles",
        list: &["iam", "roles", "list"],
        delete: &["iam", "roles", "delete"],
    },
    Sweepable {
        label: "scopes",
        list: &["iam", "scopes", "list"],
        delete: &["iam", "scopes", "delete"],
    },
    Sweepable {
        label: "groups",
        list: &["iam", "groups", "list"],
        delete: &["iam", "groups", "delete"],
    },
    // Olly rules have no name; the unique name is the rule text. A personal
    // rules list is a 403 for a team API key, which skips that entry.
    Sweepable {
        label: "olly team rules",
        list: &["olly", "rules", "list", "--team"],
        delete: &["olly", "rules", "delete", "--team"],
    },
    Sweepable {
        label: "olly personal rules",
        list: &["olly", "rules", "list"],
        delete: &["olly", "rules", "delete"],
    },
];
// Not sweepable: AI Center evaluations carry no name (only application,
// subsystem, target), so a leaked one cannot be told apart from a real one.
// Their lifecycle test retries across free slots and fails loudly when the
// pool is exhausted.

/// Which names this sweep removes: stale ones from other runs (default) or
/// everything from the current run (`CX_E2E_SWEEP_MODE=run`).
fn sweeps(name: &str) -> bool {
    let run_mode = std::env::var("CX_E2E_SWEEP_MODE").as_deref() == Ok("run");
    if run_mode {
        name.starts_with(harness::E2E_NAME_PREFIX) && name.contains(harness::run_id())
    } else {
        harness::is_stale_e2e_name(name, MAX_AGE)
    }
}

/// `::warning::` on GitHub Actions, plain stderr elsewhere.
fn warn(msg: &str) {
    if std::env::var_os("GITHUB_ACTIONS").is_some() {
        println!("::warning::{msg}");
    }
    eprintln!("[sweep] WARNING: {msg}");
}

fn str_field<'a>(item: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|k| item.get(*k).and_then(Value::as_str))
}

fn id_field(item: &Value) -> Option<String> {
    ["id", "group_id", "groupId", "name"]
        .iter()
        .find_map(|k| match item.get(*k) {
            Some(Value::String(s)) if !s.is_empty() => Some(s.clone()),
            Some(Value::Number(n)) => Some(n.to_string()),
            _ => None,
        })
}

/// Items of a list response, tolerating `{"items": [...]}`-style wrappers.
fn items(v: &Value) -> Vec<Value> {
    if let Some(arr) = v.as_array() {
        return arr.clone();
    }
    v.as_object()
        .and_then(|o| o.values().find_map(|x| x.as_array().cloned()))
        .unwrap_or_default()
}

/// The configuration list commands return a per-profile token map rather
/// than a top-level token. E2E write tests use one verified test-team profile.
fn next_page_token(v: &Value) -> Option<String> {
    v.get("next_page_tokens")?
        .as_object()?
        .values()
        .find_map(Value::as_str)
        .map(str::to_string)
}

/// Remove stale `cx-e2e-*` resources from earlier runs. Never fails the
/// suite on a single bad removal; it reports and moves on, so a flaky
/// endpoint cannot block every PR.
#[test]
#[ignore]
fn sweep_stale_e2e_resources() {
    if harness::require_write_creds("sweep_stale_e2e_resources").is_none() {
        return;
    }
    let mut swept = 0;
    let mut failed = 0;
    for s in SWEEPABLES {
        let paginated = matches!(s.label, "configuration groups" | "configuration overlays");
        let mut page_token = None;
        let mut seen_tokens = HashSet::new();
        let mut stale_resources = Vec::new();
        loop {
            let mut args: Vec<&str> = s.list.to_vec();
            if paginated {
                args.extend(["--page-size", "256"]);
                if let Some(token) = page_token.as_deref() {
                    args.extend(["--page-token", token]);
                }
            }
            args.extend(["-o", "json"]);
            let out = harness::run_raw(&args);
            if !out.status.success() {
                eprintln!("[sweep] {}: list failed, skipping", s.label);
                break;
            }
            let Some(v) = harness::parse_json(&out.stdout) else {
                eprintln!("[sweep] {}: list returned no JSON, skipping", s.label);
                break;
            };
            for item in items(&v) {
                let Some(name) = str_field(
                    &item,
                    &["name", "display_name", "displayName", "title", "rule"],
                ) else {
                    continue;
                };
                if !sweeps(name) {
                    continue;
                }
                let Some(id) = id_field(&item) else {
                    continue;
                };
                stale_resources.push((name.to_string(), id));
            }

            let Some(next_token) = paginated.then(|| next_page_token(&v)).flatten() else {
                break;
            };
            if !seen_tokens.insert(next_token.clone()) {
                eprintln!("[sweep] {}: repeated page token, stopping", s.label);
                break;
            }
            page_token = Some(next_token);
        }
        // Finish reading all pages before mutating the collection, since
        // archiving items mid-pagination can invalidate its continuation token.
        for (name, id) in stale_resources {
            let mut remove: Vec<&str> = s.delete.to_vec();
            remove.push(&id);
            remove.push("--yes");
            let out = harness::run_raw(&remove);
            if out.status.success() {
                eprintln!("[sweep] removed stale {} '{name}' ({id})", s.label);
                swept += 1;
            } else {
                eprintln!("[sweep] FAILED to remove stale {} '{name}' ({id})", s.label);
                failed += 1;
            }
        }
    }
    let (s2, f2) = sweep_nested_kinds();
    swept += s2;
    failed += f2;
    eprintln!("[sweep] done: {swept} removed, {failed} failed");
    if swept > 0 || failed > 0 {
        warn(&format!(
            "e2e sweeper found leftovers: {swept} removed, {failed} could not be removed"
        ));
    }
}

/// Resource kinds whose list is nested or whose identity is not a `name`:
/// enrichment rules (identified by `fieldName`), integration instances and
/// contextual-data instances (identified by a parameter), and extension
/// deployments (identified by the application scope). Returns
/// `(removed, failed)`.
fn sweep_nested_kinds() -> (usize, usize) {
    let mut swept = 0;
    let mut failed = 0;
    let mut del = |label: &str, name: &str, args: &[&str]| {
        let mut full: Vec<&str> = args.to_vec();
        full.push("--yes");
        if harness::run_raw(&full).status.success() {
            eprintln!("[sweep] removed stale {label} '{name}'");
            swept += 1;
        } else {
            eprintln!("[sweep] FAILED to remove stale {label} '{name}'");
            failed += 1;
        }
    };
    let list = |args: &[&str]| -> Option<Value> {
        let mut full: Vec<&str> = args.to_vec();
        full.extend(["-o", "json"]);
        let out = harness::run_raw(&full);
        out.status
            .success()
            .then(|| harness::parse_json(&out.stdout))?
    };
    let param = |item: &Value, key: &str| -> Option<String> {
        item.get("parameters")?
            .as_array()?
            .iter()
            .find(|p| p.get("key").and_then(Value::as_str) == Some(key))?
            .get("stringValue")?
            .as_str()
            .map(str::to_string)
    };

    // Enrichment rules: `{"enrichments": [{id, fieldName, ...}]}`; removed
    // by id through `enrichments remove --from-file {"enrichmentIds": [..]}`.
    if let Some(v) = list(&["enrichments", "list"]) {
        for item in items(&v) {
            let (Some(field), Some(id)) = (
                item.get("fieldName").and_then(Value::as_str),
                item.get("id").and_then(Value::as_u64),
            ) else {
                continue;
            };
            if sweeps(field) {
                let file = harness::JsonFile::new(
                    "sweep-enr",
                    &serde_json::json!({ "enrichmentIds": [id] }),
                );
                del(
                    "enrichment rule",
                    field,
                    &["enrichments", "remove", "--from-file", file.path()],
                );
            }
        }
    }

    // Integration instances of the type the lifecycle deploys, identified by
    // their `CollectorName` parameter.
    if let Some(v) = list(&["integrations", "get", "K8s-Collector"]) {
        let registered = v
            .pointer("/integrationDetail/default/registered")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for item in registered {
            let (Some(name), Some(id)) = (param(&item, "CollectorName"), id_field(&item)) else {
                continue;
            };
            if sweeps(&name) {
                del(
                    "integration instance",
                    &name,
                    &["integrations", "delete", &id],
                );
            }
        }
    }

    // Contextual-data instances, identified by `IntegrationName`.
    if let Some(v) = list(&[
        "integrations",
        "contextual-data",
        "get",
        "StatusPage-Tracker",
    ]) {
        let instances = v
            .get("instances")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for item in instances {
            let (Some(name), Some(id)) = (param(&item, "IntegrationName"), id_field(&item)) else {
                continue;
            };
            if sweeps(&name) {
                del(
                    "contextual-data instance",
                    &name,
                    &["integrations", "contextual-data", "delete", &id],
                );
            }
        }
    }

    // Extension deployments, identified by the run-unique application scope
    // the lifecycle sets; undeployed by id.
    if let Some(v) = list(&["integrations", "extensions", "deployed"]) {
        for item in items(&v) {
            let apps: Vec<&str> = item
                .get("applications")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            let Some(app) = apps.iter().find(|a| sweeps(a)) else {
                continue;
            };
            let Some(id) = id_field(&item) else {
                continue;
            };
            let file = harness::JsonFile::new("sweep-ext", &serde_json::json!({ "id": id }));
            del(
                "extension deployment",
                app,
                &[
                    "integrations",
                    "extensions",
                    "undeploy",
                    "--from-file",
                    file.path(),
                ],
            );
        }
    }

    // Custom evaluations: the CLI has no delete by design, so stale ones go
    // through the API (same helper the lifecycle test uses).
    if let Some(v) = list(&["ai-center", "custom-evaluations", "list"]) {
        for item in items(&v) {
            let (Some(name), Some(id)) = (
                str_field(&item, &["name"]).map(str::to_string),
                id_field(&item),
            ) else {
                continue;
            };
            if !sweeps(&name) {
                continue;
            }
            match crate::ai_center::delete_custom_evaluation(&id) {
                Ok(()) => {
                    eprintln!("[sweep] deleted stale custom evaluation '{name}' ({id})");
                    swept += 1;
                }
                Err(e) => {
                    eprintln!("[sweep] FAILED to delete stale custom evaluation '{name}': {e}");
                    failed += 1;
                }
            }
        }
    }

    (swept, failed)
}

// ── Pure helpers (no network) ───────────────────────────────────────────────

#[test]
fn unique_names_are_unique_and_parseable() {
    let a = harness::unique_name("x");
    let b = harness::unique_name("x");
    assert_ne!(a, b);
    assert!(a.starts_with("cx-e2e-x-"));
    assert!(a.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'));
    let stamp = harness::stamp_from_name(&a).expect("stamp parses");
    assert!((chrono::Utc::now() - stamp).num_minutes().abs() <= 1);
}

#[test]
fn stale_detection_respects_run_id_and_age() {
    let fresh = harness::unique_name("y");
    assert!(
        !harness::is_stale_e2e_name(&fresh, Duration::from_secs(0)),
        "own run is never stale"
    );
    let old = "cx-e2e-y-200001010000-deadbeef-0";
    assert!(harness::is_stale_e2e_name(old, MAX_AGE));
    let recent_other = format!(
        "cx-e2e-y-{}-deadbeef-0",
        chrono::Utc::now().format("%Y%m%d%H%M")
    );
    assert!(!harness::is_stale_e2e_name(&recent_other, MAX_AGE));
    assert!(!harness::is_stale_e2e_name("Production Alerts", MAX_AGE));
    assert!(
        harness::is_stale_e2e_name("cx-e2e-probe-DELETEME", MAX_AGE),
        "unstamped cx-e2e names are leftovers"
    );
}

#[test]
fn redact_hides_secret_fields_only() {
    let text = r#"{"id": "k-1", "name": "cx-e2e-key", "value": "cxtp_abc", "apiKey": "x"}"#;
    let out = harness::redact(text);
    assert!(out.contains(r#""name": "cx-e2e-key""#));
    assert!(out.contains(r#""value": "<redacted>""#));
    assert!(out.contains(r#""apiKey": "<redacted>""#));
    assert!(!out.contains("cxtp_abc"));
    // Key-shaped tokens are hidden in any format (text tables, TOON).
    let text = "| Value | cxtp_abcdefghijklmnopqrstuvwxyz0123 |\nvalue: cxup_ABCDEFGHIJKLMNOPQRSTUVWXYZ012345";
    let out = harness::redact(text);
    assert!(
        !out.contains("cxtp_abc") && !out.contains("cxup_ABC"),
        "{out}"
    );
    assert_eq!(out.matches("cx??_<redacted>").count(), 2);
}

#[test]
fn configuration_list_page_token_reads_profile_map() {
    let response = serde_json::json!({
        "groups": [],
        "next_page_tokens": {"staging": "opaque-token"}
    });
    assert_eq!(next_page_token(&response).as_deref(), Some("opaque-token"));
    assert_eq!(next_page_token(&serde_json::json!({"groups": []})), None);
}
