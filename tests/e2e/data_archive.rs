//! `cx archive` e2e. The archive configuration is a team-wide singleton that
//! the test team's live otel-demo traffic depends on, so every write here is
//! an idempotent round-trip of the current configuration: read it, write the
//! same value back, read it again and assert nothing changed.

use std::time::Duration;

use crate::harness;
use serde_json::{json, Value};

fn metrics_config() -> Value {
    let v = harness::run_ok_json(&["archive", "metrics", "get", "-o", "json"]);
    harness::assert_get_response(&v, &["s3", "disabled"]);
    v["tenantConfig"].clone()
}

/// `{ "s3": { bucket, region } }` for the current metrics archive, or `None`
/// when the team is not archiving metrics to S3.
fn metrics_s3_body(cfg: &Value) -> Option<Value> {
    let s3 = cfg.get("s3")?;
    let bucket = s3["bucket"].as_str()?;
    let region = s3["region"].as_str()?;
    Some(json!({ "s3": { "bucket": bucket, "region": region } }))
}

fn logs_target() -> Value {
    let v = harness::run_ok_json(&["archive", "logs", "get", "-o", "json"]);
    harness::assert_get_response(&v, &["s3", "archiveSpec"]);
    v["target"].clone()
}

#[test]
#[ignore]
fn data_archive_metrics_get() {
    if harness::require_creds("data_archive_metrics_get").is_none() {
        return;
    }
    let cfg = metrics_config();
    assert!(cfg["disabled"].is_boolean(), "tenantConfig.disabled: {cfg}");
    harness::run_ok_nonempty(&["archive", "metrics", "get"]);
    harness::run_ok_nonempty(&["archive", "metrics", "get", "-o", "toon"]);
}

#[test]
#[ignore]
fn data_archive_logs_get() {
    if harness::require_creds("data_archive_logs_get").is_none() {
        return;
    }
    let target = logs_target();
    assert!(
        target["archiveSpec"]["isActive"].is_boolean(),
        "target.archiveSpec.isActive: {target}"
    );
    harness::run_ok_nonempty(&["archive", "logs", "get"]);
    harness::run_ok_nonempty(&["archive", "logs", "get", "-o", "toon"]);
}

#[test]
#[ignore]
fn data_archive_metrics_validate_current_config() {
    if harness::require_creds("data_archive_metrics_validate_current_config").is_none() {
        return;
    }
    let Some(body) = metrics_s3_body(&metrics_config()) else {
        eprintln!("[e2e] skipping data_archive_metrics_validate_current_config: no S3 metrics archive configured");
        return;
    };
    let file = harness::JsonFile::new("archive-validate", &body);
    let v = harness::run_ok_json(&[
        "archive",
        "metrics",
        "validate",
        "--from-file",
        file.path(),
        "-o",
        "json",
    ]);
    assert!(v.is_object(), "validate should answer a JSON object: {v}");
}

/// All writes to the metrics archive singleton live in ONE test so they run
/// sequentially: `update` (same config written back) and `disable`/`enable`.
/// Another PR's run may interleave with this one, so the state asserts after
/// a write are retried for a short while instead of read once.
#[test]
#[ignore]
fn data_archive_metrics_write_roundtrips() {
    if harness::require_write_creds("data_archive_metrics_write_roundtrips").is_none() {
        return;
    }
    let before = metrics_config();
    let Some(body) = metrics_s3_body(&before) else {
        eprintln!(
            "[e2e] skipping data_archive_metrics_write_roundtrips: no S3 metrics archive configured"
        );
        return;
    };
    if before["disabled"] != json!(false) {
        // Never enable an archive somebody disabled on purpose, and do not
        // write to a config in an unknown state.
        eprintln!("[e2e] not covered: archive metrics update/disable/enable — metrics archive is currently disabled, leaving it alone");
        return;
    }

    // update: write the same S3 target back
    let file = harness::JsonFile::new("archive-update", &body);
    harness::run_write_ok(&[
        "archive",
        "metrics",
        "update",
        "--from-file",
        file.path(),
        "-o",
        "json",
    ]);
    let same = harness::eventually(10, Duration::from_secs(2), || {
        (metrics_config() == before).then_some(())
    });
    assert!(
        same.is_some(),
        "writing the same metrics archive config back must not change it: {}",
        metrics_config()
    );

    // disable, then enable. The flag is team-global and synchronous; a
    // parallel run's `enable` can land between our `disable` and the
    // read-back, so the disable+check pair is retried a few times.
    let disabled = harness::eventually(3, Duration::from_secs(1), || {
        harness::run_write_ok(&["archive", "metrics", "disable"]);
        (metrics_config()["disabled"] == json!(true)).then_some(())
    });
    assert!(disabled.is_some(), "disable should set disabled=true");
    harness::run_write_ok(&["archive", "metrics", "enable"]);
    let restored = harness::eventually(10, Duration::from_secs(2), || {
        (metrics_config() == before).then_some(())
    });
    assert!(
        restored.is_some(),
        "disable+enable must restore the configuration: {}",
        metrics_config()
    );
}

/// `archive metrics create` configures a tenant's metrics archive from
/// scratch. The test team already has a live S3 configuration, and the
/// create call replaces it (it is not an idempotent write like `update`),
/// so it is not exercised against the shared team.
#[test]
#[ignore]
fn data_archive_metrics_create_not_covered() {
    if harness::require_creds("data_archive_metrics_create_not_covered").is_none() {
        return;
    }
    eprintln!("[e2e] not covered: archive metrics create — would replace the team's live metrics archive configuration");
}

#[test]
#[ignore]
fn data_archive_logs_set_roundtrip() {
    if harness::require_write_creds("data_archive_logs_set_roundtrip").is_none() {
        return;
    }
    let before = logs_target();
    let (Some(bucket), Some(region)) = (
        before["s3"]["bucket"].as_str(),
        before["s3"]["region"].as_str(),
    ) else {
        eprintln!("[e2e] skipping data_archive_logs_set_roundtrip: no S3 logs archive configured");
        return;
    };
    let is_active = before["archiveSpec"]["isActive"].as_bool().unwrap_or(true);
    let format = before["archiveSpec"]["format"]
        .as_str()
        .unwrap_or("TARGET_FORMAT_CX_DATA_V1");
    let body = json!({
        "isActive": is_active,
        "format": format,
        "s3": { "bucket": bucket, "region": region }
    });
    let file = harness::JsonFile::new("archive-logs-set", &body);
    let v = harness::run_write_json(&[
        "archive",
        "logs",
        "set",
        "--from-file",
        file.path(),
        "-o",
        "json",
    ]);
    assert!(v.get("target").is_some(), "set should echo the target: {v}");
    let after = logs_target();
    assert_eq!(
        after, before,
        "writing the same logs archive target back must not change it"
    );
}
