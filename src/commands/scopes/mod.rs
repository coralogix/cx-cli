pub mod api;

use std::sync::Arc;

use anyhow::Result;
use colored::Colorize;
use serde_json::{json, Value};
use toon_format::encode_default as toon_encode;

use crate::config::OutputFormat;
use crate::execution::{fan_out, report_errors_and_collect_successes, ExecutionTarget};
use crate::render;
use api::{Scope, ScopesApi};

fn scope_to_json(scope: &Scope, include_profile: bool, profile: &str) -> Value {
    let mut v = json!({
        "id": scope.id,
        "name": scope.display_name(),
        "description": scope.display_description(),
        "default_expression": scope.default_expression,
        "team_id": scope.team_id,
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
        eprintln!("{}", "Reading scope definition from stdin...".dimmed());
        use std::io::Read;
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        buf
    } else {
        eprintln!(
            "{}",
            format!("Reading scope definition from {path}...").dimmed()
        );
        std::fs::read_to_string(path)?
    };
    Ok(serde_json::from_str(&raw)?)
}

pub async fn run_list(targets: &[Arc<ExecutionTarget>], output: OutputFormat) -> Result<()> {
    eprintln!("{}", "Fetching scopes...".dimmed());
    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |t| async move {
        let api = ScopesApi::new(&t.client);
        Ok(api.list().await?)
    })
    .await;

    let mut all_json: Vec<Value> = Vec::new();
    let mut all_items: Vec<(String, Scope)> = Vec::new();
    for (profile, resp) in report_errors_and_collect_successes(per_profile)? {
        for scope in resp.scopes {
            all_json.push(scope_to_json(&scope, include_profile, &profile));
            all_items.push((profile.clone(), scope));
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
                render::print_no_results("No scopes found.");
                return Ok(());
            }
            let rows: Vec<Vec<String>> = all_items
                .iter()
                .map(|(profile, scope)| {
                    vec![
                        profile.clone(),
                        scope.id.clone().unwrap_or_default(),
                        scope.display_name().to_string(),
                        scope.display_description().to_string(),
                        scope.default_expression.clone().unwrap_or_default(),
                    ]
                })
                .collect();
            render::render_table(
                &["ID", "Name", "Description", "Expression"],
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
    eprintln!("{}", format!("Fetching scope {id}...").dimmed());
    let include_profile = targets.len() > 1;
    let id = id.to_string();

    let per_profile = fan_out(targets, |t| {
        let id = id.clone();
        async move {
            let api = ScopesApi::new(&t.client);
            let resp = api.list().await?;
            let matched = resp
                .scopes
                .into_iter()
                .find(|s| s.id.as_deref() == Some(&id));
            match matched {
                Some(scope) => Ok(serde_json::to_value(json!({
                    "scope": {
                        "id": scope.id,
                        "displayName": scope.display_name,
                        "description": scope.description,
                        "defaultExpression": scope.default_expression,
                        "filters": scope.filters,
                        "teamId": scope.team_id,
                    }
                }))?),
                None => Ok(Value::Null),
            }
        }
    })
    .await;

    let mut all_results: Vec<Value> = Vec::new();
    for (profile, mut val) in report_errors_and_collect_successes(per_profile)? {
        if val.is_null() {
            eprintln!(
                "{}",
                format!("Scope {id} not found in profile '{profile}'.").yellow()
            );
        } else {
            if include_profile {
                render::tag_get_result(&mut val, &profile);
            }
            all_results.push(val);
        }
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
                "Scope not found.",
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
    eprintln!("{}", "Creating scope...".dimmed());

    let per_profile = fan_out(targets, |t| {
        let body = body.clone();
        async move {
            let api = ScopesApi::new(&t.client);
            Ok(api.create(&body).await?)
        }
    })
    .await;

    let mut all_results: Vec<Value> = Vec::new();
    for (profile, resp) in report_errors_and_collect_successes(per_profile)? {
        if let Some(scope) = resp.scope {
            let name = scope.display_name().to_string();
            render::print_created(
                "Created",
                "scope",
                Some(&name),
                scope.id.as_deref(),
                &profile,
            );
            all_results.push(scope_to_json(&scope, targets.len() > 1, &profile));
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
    eprintln!("{}", "Updating scope...".dimmed());

    let per_profile = fan_out(targets, |t| {
        let body = body.clone();
        async move {
            let api = ScopesApi::new(&t.client);
            Ok(api.update(&body).await?)
        }
    })
    .await;

    let mut all_results: Vec<Value> = Vec::new();
    for (profile, val) in report_errors_and_collect_successes(per_profile)? {
        eprintln!(
            "{}",
            format!("Updated scope in profile '{profile}'.").green()
        );
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
    eprintln!("{}", format!("Deleting scope {id}...").dimmed());
    let id = id.to_string();
    let per_profile = fan_out(targets, |t| {
        let id = id.clone();
        async move {
            let api = ScopesApi::new(&t.client);
            api.delete(&id).await?;
            Ok(())
        }
    })
    .await;
    for (profile, ()) in report_errors_and_collect_successes(per_profile)? {
        eprintln!(
            "{}",
            format!("Scope {id} deleted in profile '{profile}'.").green()
        );
    }
    Ok(())
}
