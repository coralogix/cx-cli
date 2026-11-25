pub mod api;

use std::sync::Arc;

use anyhow::Result;
use colored::Colorize;
use serde_json::Value;
use toon_format::encode_default as toon_encode;

use crate::config::OutputFormat;
use crate::execution::{fan_out, report_errors_and_collect_successes, ExecutionTarget};
use crate::render;
use api::EnrichmentsApi;

fn read_from_file(path: &str) -> Result<Value> {
    let raw = if path == "-" {
        eprintln!("{}", "Reading enrichment definition from stdin...".dimmed());
        use std::io::Read;
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        buf
    } else {
        eprintln!(
            "{}",
            format!("Reading enrichment definition from {path}...").dimmed()
        );
        std::fs::read_to_string(path)?
    };
    Ok(serde_json::from_str(&raw)?)
}

fn validate_enrichments_body(body: &Value, allow_empty: bool) -> Result<()> {
    let obj = body
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("request body must be a JSON object"))?;

    // Common mistake: sending the list/GET key `enrichments` instead of the write key.
    if obj.contains_key("enrichments") && !obj.contains_key("requestEnrichments") {
        anyhow::bail!(
            "use `requestEnrichments` (an array), not `enrichments` (that is the GET response shape)"
        );
    }

    let items = obj
        .get("requestEnrichments")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            anyhow::anyhow!("`requestEnrichments` must be an array of enrichment objects")
        })?;

    // `overwrite` (PUT) replaces all enrichments, so an empty array is a valid
    // "clear everything" request. `add` (POST) with an empty array is a no-op mistake.
    if !allow_empty && items.is_empty() {
        anyhow::bail!("`requestEnrichments` must be a non-empty array of enrichment objects");
    }

    for (i, item) in items.iter().enumerate() {
        let item_obj = item
            .as_object()
            .ok_or_else(|| anyhow::anyhow!("requestEnrichments[{i}] must be a JSON object"))?;

        if !item_obj.get("fieldName").is_some_and(Value::is_string) {
            anyhow::bail!("requestEnrichments[{i}] must include `fieldName` (string)");
        }

        // `enrichmentType` is a oneOf object (e.g. {"geoIp": {}}); list output shows it as a
        // string. We only check it is an object, not which variant.
        match item_obj.get("enrichmentType") {
            Some(Value::Object(_)) => {}
            Some(Value::String(s)) => {
                anyhow::bail!(
                    "requestEnrichments[{i}].enrichmentType must be an object, e.g. \
                     {{\"geoIp\": {{}}}} / {{\"aws\": {{\"resourceType\": \"ec2\"}}}} / \
                     {{\"customEnrichment\": {{\"id\": 1}}}} — got string \"{s}\""
                );
            }
            _ => anyhow::bail!(
                "requestEnrichments[{i}] must include `enrichmentType` (object, not string)"
            ),
        }
    }

    Ok(())
}

/// Ids for `enrichments remove`. Accepts `{"enrichmentIds": [...]}`,
/// `{"enrichment_ids": [...]}` or `{"ids": [...]}`; items may be numbers or
/// strings. The API deletes by id only, so no other shape is accepted.
fn enrichment_ids_from_body(body: &Value) -> Result<Vec<String>> {
    let obj = body
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("request body must be a JSON object"))?;
    let items = ["enrichmentIds", "enrichment_ids", "ids"]
        .iter()
        .find_map(|key| obj.get(*key))
        .and_then(Value::as_array)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "body must include `enrichmentIds` (array of enrichment rule ids, as shown \
                 by `cx enrichments list`)"
            )
        })?;
    if items.is_empty() {
        anyhow::bail!("`enrichmentIds` must be a non-empty array of enrichment rule ids");
    }
    items
        .iter()
        .enumerate()
        .map(|(i, item)| match item {
            Value::Number(n) => Ok(n.to_string()),
            Value::String(s) if !s.trim().is_empty() => Ok(s.trim().to_string()),
            other => anyhow::bail!("enrichmentIds[{i}] must be a number or string, got {other}"),
        })
        .collect()
}

fn render_results(
    all_results: &[Value],
    output: OutputFormat,
    include_profile: bool,
) -> Result<()> {
    match output {
        OutputFormat::Json => render::render_json_auto(all_results)?,
        OutputFormat::Toon => {
            let toon = toon_encode(&all_results)
                .map_err(|e| anyhow::anyhow!("TOON encoding failed: {e}"))?;
            println!("{toon}");
        }
        OutputFormat::Text => {
            if all_results.is_empty() {
                render::print_no_results("No data found.");
                return Ok(());
            }
            if include_profile {
                for val in all_results {
                    println!("{}", serde_json::to_string_pretty(val)?);
                }
            } else if let Some(val) = all_results.first() {
                println!("{}", serde_json::to_string_pretty(val)?);
            }
        }
    }
    Ok(())
}

pub async fn run_list(targets: &[Arc<ExecutionTarget>], output: OutputFormat) -> Result<()> {
    eprintln!("{}", "Fetching enrichments...".dimmed());
    let include_profile = targets.len() > 1;
    let per_profile = fan_out(targets, |t| async move {
        let api = EnrichmentsApi::new(&t.client);
        Ok(api.list().await?)
    })
    .await;
    let mut all_results: Vec<Value> = Vec::new();
    for (profile, mut val) in report_errors_and_collect_successes(per_profile)? {
        if include_profile {
            render::tag_get_result(&mut val, &profile);
        }
        crate::execution::emit_console_link_for_profile(targets, &profile, |b| {
            crate::console_url::enrichments_url(b)
        })
        .await;
        all_results.push(val);
    }
    render_results(&all_results, output, include_profile)
}

pub async fn run_add(
    targets: &[Arc<ExecutionTarget>],
    from_file: &str,
    output: OutputFormat,
) -> Result<()> {
    let body = read_from_file(from_file)?;
    validate_enrichments_body(&body, false)?;
    eprintln!("{}", "Adding enrichment rules...".dimmed());
    let per_profile = fan_out(targets, |t| {
        let body = body.clone();
        async move {
            let api = EnrichmentsApi::new(&t.client);
            Ok(api.add(&body).await?)
        }
    })
    .await;
    let mut all_results: Vec<Value> = Vec::new();
    for (profile, val) in report_errors_and_collect_successes(per_profile)? {
        eprintln!(
            "{}",
            format!("Added enrichments in profile '{profile}'.").green()
        );
        crate::execution::emit_console_link_for_profile(targets, &profile, |b| {
            crate::console_url::enrichments_url(b)
        })
        .await;
        all_results.push(val);
    }
    render_results(&all_results, output, targets.len() > 1)
}

pub async fn run_remove(
    targets: &[Arc<ExecutionTarget>],
    from_file: &str,
    output: OutputFormat,
) -> Result<()> {
    let body = read_from_file(from_file)?;
    let ids = enrichment_ids_from_body(&body)?;
    eprintln!("{}", "Removing enrichment rules...".dimmed());
    let per_profile = fan_out(targets, |t| {
        let ids = ids.clone();
        async move {
            let api = EnrichmentsApi::new(&t.client);
            Ok(api.remove(&ids).await?)
        }
    })
    .await;
    let mut all_results: Vec<Value> = Vec::new();
    for (profile, val) in report_errors_and_collect_successes(per_profile)? {
        eprintln!(
            "{}",
            format!("Removed enrichments in profile '{profile}'.").green()
        );
        crate::execution::emit_console_link_for_profile(targets, &profile, |b| {
            crate::console_url::enrichments_url(b)
        })
        .await;
        all_results.push(val);
    }
    render_results(&all_results, output, targets.len() > 1)
}

pub async fn run_overwrite(
    targets: &[Arc<ExecutionTarget>],
    from_file: &str,
    output: OutputFormat,
) -> Result<()> {
    let body = read_from_file(from_file)?;
    validate_enrichments_body(&body, true)?;
    eprintln!("{}", "Overwriting enrichment rules...".dimmed());
    let per_profile = fan_out(targets, |t| {
        let body = body.clone();
        async move {
            let api = EnrichmentsApi::new(&t.client);
            Ok(api.overwrite(&body).await?)
        }
    })
    .await;
    let mut all_results: Vec<Value> = Vec::new();
    for (profile, val) in report_errors_and_collect_successes(per_profile)? {
        eprintln!(
            "{}",
            format!("Overwrote enrichments in profile '{profile}'.").green()
        );
        crate::execution::emit_console_link_for_profile(targets, &profile, |b| {
            crate::console_url::enrichments_url(b)
        })
        .await;
        all_results.push(val);
    }
    render_results(&all_results, output, targets.len() > 1)
}

pub async fn run_limit(targets: &[Arc<ExecutionTarget>], output: OutputFormat) -> Result<()> {
    eprintln!("{}", "Fetching enrichment limits...".dimmed());
    let include_profile = targets.len() > 1;
    let per_profile = fan_out(targets, |t| async move {
        let api = EnrichmentsApi::new(&t.client);
        Ok(api.limit().await?)
    })
    .await;
    let mut all_results: Vec<Value> = Vec::new();
    for (profile, mut val) in report_errors_and_collect_successes(per_profile)? {
        if include_profile {
            render::tag_get_result(&mut val, &profile);
        }
        crate::execution::emit_console_link_for_profile(targets, &profile, |b| {
            crate::console_url::enrichments_url(b)
        })
        .await;
        all_results.push(val);
    }
    render_results(&all_results, output, include_profile)
}

pub async fn run_settings(targets: &[Arc<ExecutionTarget>], output: OutputFormat) -> Result<()> {
    eprintln!("{}", "Fetching enrichment settings...".dimmed());
    let include_profile = targets.len() > 1;
    let per_profile = fan_out(targets, |t| async move {
        let api = EnrichmentsApi::new(&t.client);
        Ok(api.settings().await?)
    })
    .await;
    let mut all_results: Vec<Value> = Vec::new();
    for (profile, mut val) in report_errors_and_collect_successes(per_profile)? {
        if include_profile {
            render::tag_get_result(&mut val, &profile);
        }
        crate::execution::emit_console_link_for_profile(targets, &profile, |b| {
            crate::console_url::enrichments_url(b)
        })
        .await;
        all_results.push(val);
    }
    render_results(&all_results, output, include_profile)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn valid_body() -> Value {
        json!({
            "requestEnrichments": [
                {
                    "fieldName": "sourceIPs",
                    "enrichmentType": { "geoIp": { "withAsn": true } }
                }
            ]
        })
    }

    #[test]
    fn validate_enrichments_body_accepts_valid() {
        validate_enrichments_body(&valid_body(), false).unwrap();
        validate_enrichments_body(&valid_body(), true).unwrap();
    }

    #[test]
    fn validate_enrichments_body_rejects_enrichments_key() {
        let body =
            json!({ "enrichments": [{ "fieldName": "x", "enrichmentType": { "geoIp": {} } }] });
        let err = validate_enrichments_body(&body, false).unwrap_err();
        assert!(err.to_string().contains("requestEnrichments"));
        assert!(err.to_string().contains("enrichments"));
    }

    #[test]
    fn validate_enrichments_body_rejects_string_enrichment_type() {
        let body = json!({
            "requestEnrichments": [
                { "fieldName": "sourceIPs", "enrichmentType": "GEO_IP" }
            ]
        });
        let err = validate_enrichments_body(&body, false).unwrap_err();
        assert!(err.to_string().contains("enrichmentType"));
        assert!(err.to_string().contains("object"));
    }

    #[test]
    fn validate_enrichments_body_rejects_missing_field_name() {
        let body = json!({
            "requestEnrichments": [{ "enrichmentType": { "geoIp": {} } }]
        });
        let err = validate_enrichments_body(&body, false).unwrap_err();
        assert!(err.to_string().contains("fieldName"));
    }

    #[test]
    fn validate_enrichments_body_rejects_empty_array_when_not_allowed() {
        let body = json!({ "requestEnrichments": [] });
        let err = validate_enrichments_body(&body, false).unwrap_err();
        assert!(err.to_string().contains("non-empty"));
    }

    #[test]
    fn validate_enrichments_body_accepts_empty_array_when_allowed() {
        // `overwrite` uses an empty array to clear all enrichment rules.
        let body = json!({ "requestEnrichments": [] });
        validate_enrichments_body(&body, true).unwrap();
    }

    #[test]
    fn validate_enrichments_body_rejects_non_object() {
        let err = validate_enrichments_body(&json!([1, 2, 3]), false).unwrap_err();
        assert!(err.to_string().contains("JSON object"));
    }

    #[test]
    fn validate_enrichments_body_rejects_non_object_item() {
        let body = json!({ "requestEnrichments": ["not-an-object"] });
        let err = validate_enrichments_body(&body, false).unwrap_err();
        assert!(err.to_string().contains("requestEnrichments[0]"));
        assert!(err.to_string().contains("JSON object"));
    }

    #[test]
    fn validate_enrichments_body_rejects_missing_enrichment_type() {
        let body = json!({ "requestEnrichments": [{ "fieldName": "sourceIPs" }] });
        let err = validate_enrichments_body(&body, false).unwrap_err();
        assert!(err.to_string().contains("enrichmentType"));
    }

    #[test]
    fn enrichment_ids_accepts_numbers_and_strings_under_any_key() {
        let ids =
            enrichment_ids_from_body(&json!({ "enrichmentIds": [272627, "272628"] })).unwrap();
        assert_eq!(ids, vec!["272627", "272628"]);
        let ids = enrichment_ids_from_body(&json!({ "enrichment_ids": [1] })).unwrap();
        assert_eq!(ids, vec!["1"]);
        let ids = enrichment_ids_from_body(&json!({ "ids": [" 7 "] })).unwrap();
        assert_eq!(ids, vec!["7"]);
    }

    #[test]
    fn enrichment_ids_rejects_bad_shapes() {
        assert!(enrichment_ids_from_body(&json!([1])).is_err());
        let err = enrichment_ids_from_body(&json!({ "requestEnrichments": [] })).unwrap_err();
        assert!(err.to_string().contains("enrichmentIds"));
        assert!(enrichment_ids_from_body(&json!({ "enrichmentIds": [] })).is_err());
        assert!(enrichment_ids_from_body(&json!({ "enrichmentIds": [true] })).is_err());
        assert!(enrichment_ids_from_body(&json!({ "enrichmentIds": [""] })).is_err());
    }
}
