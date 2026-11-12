//! Stale-resource sweeper for the writable e2e team.
//!
//! Every write test names its resources with [`harness::unique_name`] and
//! deletes them on exit (also on panic, through [`harness::Cleanup`]). A
//! killed runner or a lost network can still leave resources behind, so CI
//! runs this sweeper before the suite. It lists each resource type, keeps
//! only names produced by `unique_name` in a *different* run that are older
//! than [`MAX_AGE`], and deletes those. Resources from the current run and
//! anything not named `cx-e2e-...` are never touched.

use std::time::Duration;

use serde_json::Value;

use crate::harness;

/// Resources older than this (per the stamp in their name) are considered
/// leftovers. Longer than any CI job timeout, so a concurrent run can never
/// have its live resources swept.
const MAX_AGE: Duration = Duration::from_secs(2 * 60 * 60);

/// One sweepable resource type: how to list it and how to delete one.
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
    Sweepable {
        label: "ai-center evaluations",
        list: &["ai-center", "evaluations", "list"],
        delete: &["ai-center", "evaluations", "delete"],
    },
];

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

/// Delete stale `cx-e2e-*` resources from earlier runs. Never fails the
/// suite on a single bad delete; it reports and moves on, so a flaky
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
        let mut args: Vec<&str> = s.list.to_vec();
        args.extend(["-o", "json"]);
        let out = harness::run_raw(&args);
        if !out.status.success() {
            eprintln!("[sweep] {}: list failed, skipping", s.label);
            continue;
        }
        let Some(v) = harness::parse_json(&out.stdout) else {
            eprintln!("[sweep] {}: list returned no JSON, skipping", s.label);
            continue;
        };
        for item in items(&v) {
            let Some(name) = str_field(&item, &["name", "display_name", "displayName", "title"])
            else {
                continue;
            };
            if !harness::is_stale_e2e_name(name, MAX_AGE) {
                continue;
            }
            let Some(id) = id_field(&item) else {
                continue;
            };
            let mut del: Vec<&str> = s.delete.to_vec();
            del.push(&id);
            del.push("--yes");
            let out = harness::run_raw(&del);
            if out.status.success() {
                eprintln!("[sweep] deleted stale {} '{name}' ({id})", s.label);
                swept += 1;
            } else {
                eprintln!("[sweep] FAILED to delete stale {} '{name}' ({id})", s.label);
                failed += 1;
            }
        }
    }
    eprintln!("[sweep] done: {swept} deleted, {failed} failed");
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
