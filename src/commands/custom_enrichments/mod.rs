pub mod api;

use std::sync::Arc;

use anyhow::Result;
use colored::Colorize;
use serde_json::{json, Value};
use toon_format::encode_default as toon_encode;

use crate::config::OutputFormat;
use crate::execution::{fan_out, report_errors_and_collect_successes, ExecutionTarget};
use crate::render;
use api::{CustomEnrichment, CustomEnrichmentsApi};

fn ce_to_json(ce: &CustomEnrichment, include_profile: bool, profile: &str) -> Value {
    let mut v = json!({
        "id": ce.id,
        "name": ce.display_name(),
        "description": ce.description,
        "type": ce.display_type(),
        "create_time": ce.create_time,
    });
    if include_profile {
        if let Value::Object(ref mut m) = v {
            m.insert("profile".to_string(), Value::String(profile.to_string()));
        }
    }
    v
}

fn read_from_file(path: &str) -> Result<Value> {
    let raw = if path == "-" {
        eprintln!(
            "{}",
            "Reading custom enrichment definition from stdin...".dimmed()
        );
        use std::io::Read;
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        buf
    } else {
        eprintln!(
            "{}",
            format!("Reading custom enrichment definition from {path}...").dimmed()
        );
        std::fs::read_to_string(path)?
    };
    Ok(serde_json::from_str(&raw)?)
}

fn validate_file_field(body: &serde_json::Map<String, Value>, context: &str) -> Result<()> {
    match body.get("file") {
        Some(Value::Object(_)) => Ok(()),
        Some(Value::String(_)) => anyhow::bail!(
            "{context}: `file` must be an object `{{textual, extension, name, size}}` \
             (the v5 JSON API, not multipart `file=@...`)"
        ),
        _ => anyhow::bail!(
            "{context}: must include `file` (object with textual, extension, name, size)"
        ),
    }
}

/// Shallow validation of the CreateCustomEnrichment request body.
fn validate_create_body(body: &Value) -> Result<()> {
    let ctx = "create custom enrichment";
    let obj = body
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("{ctx}: request body must be a JSON object"))?;

    if !obj.get("name").is_some_and(Value::is_string) {
        anyhow::bail!("{ctx}: must include `name` (string)");
    }
    if !obj.get("description").is_some_and(Value::is_string) {
        anyhow::bail!("{ctx}: must include `description` (string)");
    }
    validate_file_field(obj, ctx)?;
    Ok(())
}

/// Shallow validation of the UpdateCustomEnrichment request body.
/// Reduce the `POST /all/contents` answer (every table, full content) to the
/// rows of table `id` that contain `query` (case-insensitive). The first
/// line of the table is its header and is returned separately.
fn filter_search_results(resp: &Value, id: &str, query: &str) -> Result<Value> {
    let tables = resp
        .get("customEnrichmentsData")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("unexpected search response: {resp}"))?;
    let table = tables
        .iter()
        .find(|t| {
            // Ids are numbers on the wire; `--id` is text.
            match t.pointer("/definition/id") {
                Some(Value::String(s)) => s == id,
                Some(Value::Number(n)) => id.parse::<u64>().ok() == n.as_u64(),
                _ => false,
            }
        })
        .ok_or_else(|| anyhow::anyhow!("custom enrichment {id} not found"))?;
    let textual = table.get("textual").and_then(Value::as_str).unwrap_or("");
    let mut lines = textual.lines();
    let header = lines.next().unwrap_or("").to_string();
    let needle = query.to_lowercase();
    let matches: Vec<Value> = lines
        .filter(|line| line.to_lowercase().contains(&needle))
        .map(|line| Value::String(line.to_string()))
        .collect();
    Ok(json!({
        "customEnrichmentId": table.pointer("/definition/id").cloned().unwrap_or(Value::Null),
        "name": table.pointer("/definition/name").cloned().unwrap_or(Value::Null),
        "query": query,
        "header": header,
        "matchCount": matches.len(),
        "matches": matches,
    }))
}

fn validate_update_body(body: &Value) -> Result<()> {
    let ctx = "update custom enrichment";
    let obj = body
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("{ctx}: request body must be a JSON object"))?;

    if !obj.get("customEnrichmentId").is_some_and(Value::is_number) {
        anyhow::bail!("{ctx}: must include `customEnrichmentId` (number)");
    }
    if !obj.get("name").is_some_and(Value::is_string) {
        anyhow::bail!("{ctx}: must include `name` (string)");
    }
    if !obj.get("description").is_some_and(Value::is_string) {
        anyhow::bail!("{ctx}: must include `description` (string)");
    }
    validate_file_field(obj, ctx)?;
    Ok(())
}

pub async fn run_list(targets: &[Arc<ExecutionTarget>], output: OutputFormat) -> Result<()> {
    eprintln!("{}", "Fetching custom enrichments...".dimmed());
    let include_profile = targets.len() > 1;
    let per_profile = fan_out(targets, |t| async move {
        let api = CustomEnrichmentsApi::new(&t.client);
        Ok(api.list().await?)
    })
    .await;
    let mut all_json: Vec<Value> = Vec::new();
    let mut all_items: Vec<(String, CustomEnrichment)> = Vec::new();
    for (profile, resp) in report_errors_and_collect_successes(per_profile)? {
        // Print the enrichments page link to stderr once per profile.
        // Skip when there are no tables, since there's nothing to view.
        if !resp.custom_enrichments.is_empty() {
            crate::execution::emit_console_link_for_profile(targets, &profile, |b| {
                crate::console_url::enrichments_url(b)
            })
            .await;
        }
        for ce in resp.custom_enrichments {
            let ce_json = ce_to_json(&ce, include_profile, &profile);
            all_json.push(ce_json);
            all_items.push((profile.clone(), ce));
        }
    }
    match output {
        OutputFormat::Json => render::render_json(&all_json)?,
        OutputFormat::Toon => {
            let toon =
                toon_encode(&all_json).map_err(|e| anyhow::anyhow!("TOON encoding failed: {e}"))?;
            println!("{toon}");
        }
        OutputFormat::Text => {
            if all_items.is_empty() {
                render::print_no_results("No custom enrichments found.");
                return Ok(());
            }
            let rows: Vec<Vec<String>> = all_items
                .iter()
                .map(|(profile, ce)| {
                    vec![
                        profile.clone(),
                        ce.id.clone().unwrap_or_default(),
                        ce.display_name().to_string(),
                        ce.description.clone().unwrap_or_default(),
                        ce.display_type(),
                        ce.create_time.clone().unwrap_or_default(),
                    ]
                })
                .collect();
            render::render_table(
                &["ID", "Name", "Description", "Type", "Created"],
                rows,
                include_profile,
            );
        }
    }
    Ok(())
}

pub async fn run_get(
    targets: &[Arc<ExecutionTarget>],
    id: &str,
    output: OutputFormat,
) -> Result<()> {
    eprintln!("{}", format!("Fetching custom enrichment {id}...").dimmed());
    let include_profile = targets.len() > 1;
    let id = id.to_string();
    let per_profile = fan_out(targets, |t| {
        let id = id.clone();
        async move {
            let api = CustomEnrichmentsApi::new(&t.client);
            Ok(api.get(&id).await?)
        }
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
    match output {
        OutputFormat::Json => render::render_json_auto(&all_results)?,
        OutputFormat::Toon => {
            let toon = toon_encode(&all_results)
                .map_err(|e| anyhow::anyhow!("TOON encoding failed: {e}"))?;
            println!("{toon}");
        }
        OutputFormat::Text => {
            render::render_get_text(
                &all_results,
                include_profile,
                "Custom enrichment not found.",
                None::<&dyn Fn(&Value)>,
            )?;
        }
    }
    Ok(())
}

pub async fn run_create(
    targets: &[Arc<ExecutionTarget>],
    from_file: &str,
    output: OutputFormat,
) -> Result<()> {
    let body = read_from_file(from_file)?;
    validate_create_body(&body)?;
    eprintln!("{}", "Creating custom enrichment...".dimmed());
    let include_profile = targets.len() > 1;
    let per_profile = fan_out(targets, |t| {
        let body = body.clone();
        async move {
            let api = CustomEnrichmentsApi::new(&t.client);
            Ok(api.create(&body).await?)
        }
    })
    .await;
    let mut all_results: Vec<Value> = Vec::new();
    for (profile, resp) in report_errors_and_collect_successes(per_profile)? {
        if let Some(ce) = resp.custom_enrichment {
            eprintln!(
                "{}",
                format!(
                    "Created custom enrichment '{}' in profile '{profile}'.",
                    ce.display_name()
                )
                .green()
            );
            let ce_json = ce_to_json(&ce, include_profile, &profile);
            crate::execution::emit_console_link_for_profile(
                targets,
                &profile,
                crate::console_url::enrichments_url,
            )
            .await;
            all_results.push(ce_json);
        }
    }
    match output {
        OutputFormat::Json => render::render_json_auto(&all_results)?,
        OutputFormat::Toon => {
            let toon = toon_encode(&all_results)
                .map_err(|e| anyhow::anyhow!("TOON encoding failed: {e}"))?;
            println!("{toon}");
        }
        OutputFormat::Text => {}
    }
    Ok(())
}

pub async fn run_update(
    targets: &[Arc<ExecutionTarget>],
    from_file: &str,
    output: OutputFormat,
) -> Result<()> {
    let body = read_from_file(from_file)?;
    validate_update_body(&body)?;
    eprintln!("{}", "Updating custom enrichment...".dimmed());
    let per_profile = fan_out(targets, |t| {
        let body = body.clone();
        async move {
            let api = CustomEnrichmentsApi::new(&t.client);
            Ok(api.update(&body).await?)
        }
    })
    .await;
    let mut all_results: Vec<Value> = Vec::new();
    for (profile, val) in report_errors_and_collect_successes(per_profile)? {
        eprintln!(
            "{}",
            format!("Updated custom enrichment in profile '{profile}'.").green()
        );
        crate::execution::emit_console_link_for_profile(targets, &profile, |b| {
            crate::console_url::enrichments_url(b)
        })
        .await;
        all_results.push(val);
    }
    match output {
        OutputFormat::Json => render::render_json_auto(&all_results)?,
        OutputFormat::Toon => {
            let toon = toon_encode(&all_results)
                .map_err(|e| anyhow::anyhow!("TOON encoding failed: {e}"))?;
            println!("{toon}");
        }
        OutputFormat::Text => {}
    }
    Ok(())
}

pub async fn run_delete(targets: &[Arc<ExecutionTarget>], id: &str) -> Result<()> {
    eprintln!("{}", format!("Deleting custom enrichment {id}...").dimmed());
    let id = id.to_string();
    let per_profile = fan_out(targets, |t| {
        let id = id.clone();
        async move {
            let api = CustomEnrichmentsApi::new(&t.client);
            api.delete(&id).await?;
            Ok(())
        }
    })
    .await;
    for (profile, ()) in report_errors_and_collect_successes(per_profile)? {
        eprintln!(
            "{}",
            format!("Custom enrichment {id} deleted in profile '{profile}'.").green()
        );
        crate::execution::emit_console_link_for_profile(targets, &profile, |b| {
            crate::console_url::enrichments_url(b)
        })
        .await;
    }
    Ok(())
}

pub async fn run_search(
    targets: &[Arc<ExecutionTarget>],
    id: &str,
    query: &str,
    output: OutputFormat,
) -> Result<()> {
    eprintln!(
        "{}",
        format!("Searching custom enrichment {id}...").dimmed()
    );
    let id = id.to_string();
    let query = query.to_string();
    let per_profile = fan_out(targets, |t| {
        let id = id.clone();
        let query = query.clone();
        async move {
            let api = CustomEnrichmentsApi::new(&t.client);
            Ok(api.search(&id, &query).await?)
        }
    })
    .await;
    let mut all_results: Vec<Value> = Vec::new();
    for (profile, val) in report_errors_and_collect_successes(per_profile)? {
        let filtered = filter_search_results(&val, &id, &query)?;
        crate::execution::emit_console_link_for_profile(targets, &profile, |b| {
            crate::console_url::enrichments_url(b)
        })
        .await;
        all_results.push(filtered);
    }
    match output {
        OutputFormat::Json => render::render_json_auto(&all_results)?,
        OutputFormat::Toon => {
            let toon = toon_encode(&all_results)
                .map_err(|e| anyhow::anyhow!("TOON encoding failed: {e}"))?;
            println!("{toon}");
        }
        OutputFormat::Text => {
            for val in &all_results {
                println!("{}", serde_json::to_string_pretty(val)?);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn valid_create_body() -> Value {
        json!({
            "name": "IP Lookup",
            "description": "Maps IPs to locations",
            "file": {
                "textual": "ip,city\n1.2.3.4,London",
                "extension": "csv",
                "name": "lookup.csv",
                "size": 24
            }
        })
    }

    fn valid_update_body() -> Value {
        json!({
            "customEnrichmentId": 1,
            "name": "IP Lookup",
            "description": "Maps IPs to locations",
            "file": {
                "textual": "ip,city\n1.2.3.4,London",
                "extension": "csv",
                "name": "lookup.csv",
                "size": 24
            }
        })
    }

    #[test]
    fn validate_create_body_accepts_valid() {
        validate_create_body(&valid_create_body()).unwrap();
    }

    #[test]
    fn validate_create_body_rejects_missing_name() {
        let mut body = valid_create_body();
        body.as_object_mut().unwrap().remove("name");
        let err = validate_create_body(&body).unwrap_err();
        assert!(err.to_string().contains("name"));
    }

    #[test]
    fn validate_create_body_rejects_missing_description() {
        let mut body = valid_create_body();
        body.as_object_mut().unwrap().remove("description");
        let err = validate_create_body(&body).unwrap_err();
        assert!(err.to_string().contains("description"));
    }

    #[test]
    fn validate_create_body_rejects_string_file() {
        let body = json!({
            "name": "IP Lookup",
            "description": "Maps IPs",
            "file": "/path/to/file.csv"
        });
        let err = validate_create_body(&body).unwrap_err();
        assert!(err.to_string().contains("file"));
        assert!(err.to_string().contains("object"));
    }

    #[test]
    fn validate_create_body_rejects_missing_file() {
        let mut body = valid_create_body();
        body.as_object_mut().unwrap().remove("file");
        let err = validate_create_body(&body).unwrap_err();
        assert!(err.to_string().contains("file"));
    }

    #[test]
    fn validate_create_body_rejects_non_object() {
        let err = validate_create_body(&json!([1, 2, 3])).unwrap_err();
        assert!(err.to_string().contains("JSON object"));
        // Context disambiguates which command rejected the payload.
        assert!(err.to_string().contains("create"));
    }

    #[test]
    fn validate_update_body_accepts_valid() {
        validate_update_body(&valid_update_body()).unwrap();
    }

    #[test]
    fn validate_update_body_rejects_missing_custom_enrichment_id() {
        let body = valid_create_body();
        let err = validate_update_body(&body).unwrap_err();
        assert!(err.to_string().contains("customEnrichmentId"));
    }

    #[test]
    fn validate_update_body_rejects_string_file() {
        let mut body = valid_update_body();
        body.as_object_mut()
            .unwrap()
            .insert("file".to_string(), json!("/path/to/file.csv"));
        let err = validate_update_body(&body).unwrap_err();
        assert!(err.to_string().contains("file"));
        assert!(err.to_string().contains("object"));
    }

    #[test]
    fn validate_update_body_error_is_scoped_to_update() {
        let mut body = valid_update_body();
        body.as_object_mut().unwrap().remove("file");
        let err = validate_update_body(&body).unwrap_err();
        assert!(err.to_string().contains("update"));
    }

    fn search_response() -> Value {
        json!({
            "customEnrichmentsData": [
                {
                    "definition": { "id": 172830, "name": "ip_lookup", "version": 1 },
                    "textual": "ip,city\n1.2.3.4,London\n5.6.7.8,Paris"
                },
                {
                    "definition": { "id": 999, "name": "other" },
                    "textual": "k,v\nLondon,other-table"
                }
            ]
        })
    }

    #[test]
    fn filter_search_results_matches_rows_of_the_requested_table_only() {
        let out = filter_search_results(&search_response(), "172830", "london").unwrap();
        assert_eq!(out["customEnrichmentId"], json!(172830));
        assert_eq!(out["name"], json!("ip_lookup"));
        assert_eq!(out["header"], json!("ip,city"));
        assert_eq!(out["matchCount"], json!(1));
        assert_eq!(out["matches"], json!(["1.2.3.4,London"]));
    }

    #[test]
    fn filter_search_results_no_match_is_empty() {
        let out = filter_search_results(&search_response(), "172830", "zzz").unwrap();
        assert_eq!(out["matchCount"], json!(0));
        assert_eq!(out["matches"], json!([]));
    }

    #[test]
    fn filter_search_results_unknown_table_errors() {
        let err = filter_search_results(&search_response(), "1", "x").unwrap_err();
        assert!(err.to_string().contains("not found"));
        let err = filter_search_results(&json!({}), "1", "x").unwrap_err();
        assert!(err.to_string().contains("unexpected"));
    }
}
