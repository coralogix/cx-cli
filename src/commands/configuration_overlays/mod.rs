pub mod api;

use std::sync::Arc;

use anyhow::{bail, Result};
use colored::Colorize;
use serde_json::{json, Map, Value};
use toon_format::encode_default as toon_encode;

use crate::config::OutputFormat;
use crate::execution::{fan_out, report_errors_and_collect_successes, ExecutionTarget};
use crate::render;
use api::ConfigurationOverlaysApi;

fn read_from_file(path: &str) -> Result<Value> {
    let raw = if path == "-" {
        eprintln!(
            "{}",
            "Reading configuration overlay definition from stdin...".dimmed()
        );
        use std::io::Read;
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        buf
    } else {
        eprintln!(
            "{}",
            format!("Reading configuration overlay definition from {path}...").dimmed()
        );
        std::fs::read_to_string(path)?
    };
    Ok(serde_json::from_str(&raw)?)
}

fn field_text(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(value)) => value.clone(),
        Some(Value::Null) | None => String::new(),
        Some(value) => value.to_string(),
    }
}

fn overlay_row(overlay: &Value, profile: &str) -> Vec<String> {
    vec![
        profile.to_string(),
        field_text(overlay.get("id")),
        field_text(overlay.get("name")),
        field_text(overlay.get("priorityOrder")),
        field_text(overlay.get("active")),
        field_text(overlay.get("archived")),
        field_text(overlay.get("version")),
        overlay
            .pointer("/custom/targets")
            .and_then(Value::as_array)
            .map(|targets| targets.len().to_string())
            .unwrap_or_default(),
        overlay
            .get("schedules")
            .and_then(Value::as_array)
            .map(|schedules| schedules.len().to_string())
            .unwrap_or_default(),
    ]
}

fn tag_profile(overlay: &mut Value, include_profile: bool, profile: &str) {
    if include_profile {
        if let Value::Object(object) = overlay {
            object.insert("profile".to_string(), Value::String(profile.to_string()));
        }
    }
}

fn next_page_command(
    profile: &str,
    include_archived: bool,
    page_size: Option<u16>,
    page_token: &str,
) -> String {
    fn quote(value: &str) -> String {
        format!("'{}'", value.replace('\'', "'\\''"))
    }

    let mut args = vec![
        "cx".to_string(),
        "--profile".to_string(),
        quote(profile),
        "configuration-overlays".to_string(),
        "list".to_string(),
    ];
    if include_archived {
        args.push("--include-archived".to_string());
    }
    if let Some(page_size) = page_size {
        args.extend(["--page-size".to_string(), page_size.to_string()]);
    }
    args.extend(["--page-token".to_string(), quote(page_token)]);
    args.join(" ")
}

fn validate_page_token_for_profiles(profile_count: usize, page_token: Option<&str>) -> Result<()> {
    if profile_count > 1 && page_token.is_some() {
        bail!(
            "`--page-token` is profile-specific. Continue with a separate `--profile <name>` command for each profile's token."
        );
    }
    Ok(())
}

pub async fn run_list(
    targets: &[Arc<ExecutionTarget>],
    include_archived: bool,
    page_size: Option<u16>,
    page_token: Option<&str>,
    output: OutputFormat,
) -> Result<()> {
    validate_page_token_for_profiles(targets.len(), page_token)?;
    eprintln!("{}", "Fetching configuration overlays...".dimmed());
    let include_profile = targets.len() > 1;
    let page_token = page_token.map(str::to_string);
    let per_profile = fan_out(targets, |target| {
        let page_token = page_token.clone();
        async move {
            let api = ConfigurationOverlaysApi::new(&target.client);
            Ok(api
                .list(include_archived, page_size, page_token.as_deref())
                .await?)
        }
    })
    .await;

    let mut all_json = Vec::new();
    let mut all_rows = Vec::new();
    let mut next_page_tokens = Map::new();
    for (profile, response) in report_errors_and_collect_successes(per_profile)? {
        if let Some(token) = response.next_page_token.filter(|token| !token.is_empty()) {
            next_page_tokens.insert(profile.clone(), Value::String(token));
        }
        for mut overlay in response.overlays {
            tag_profile(&mut overlay, include_profile, &profile);
            all_rows.push((profile.clone(), overlay_row(&overlay, &profile)));
            all_json.push(overlay);
        }
    }

    if !next_page_tokens.is_empty() {
        eprintln!(
            "{}",
            "More configuration overlays are available. Continue separately for each profile, reusing the same --include-archived and --page-size options; next-page tokens are included in JSON and TOON output."
                .yellow()
        );
    }

    match output {
        OutputFormat::Json | OutputFormat::Toon => {
            let mut response = json!({ "overlays": all_json });
            if !next_page_tokens.is_empty() {
                response["next_page_tokens"] = Value::Object(next_page_tokens);
            }
            if output == OutputFormat::Json {
                render::render_json_auto(&[response])?;
            } else {
                let toon = toon_encode(&response)
                    .map_err(|error| anyhow::anyhow!("TOON encoding failed: {error}"))?;
                println!("{toon}");
            }
        }
        OutputFormat::Text => {
            if all_rows.is_empty() {
                render::print_no_results("No configuration overlays found.");
                return Ok(());
            }
            render::render_table(
                &[
                    "ID",
                    "Name",
                    "Priority",
                    "Active",
                    "Archived",
                    "Version",
                    "Targets",
                    "Schedules",
                ],
                all_rows.into_iter().map(|(_, row)| row).collect(),
                include_profile,
            );
            for (profile, token) in next_page_tokens {
                let token = token
                    .as_str()
                    .expect("next-page tokens are stored as strings");
                eprintln!(
                    "{}",
                    format!(
                        "Next page for profile '{profile}': {}",
                        next_page_command(&profile, include_archived, page_size, token)
                    )
                    .dimmed()
                );
            }
        }
    }
    Ok(())
}

pub async fn run_get(
    targets: &[Arc<ExecutionTarget>],
    id: &str,
    include_archived: bool,
    output: OutputFormat,
) -> Result<()> {
    eprintln!(
        "{}",
        format!("Fetching configuration overlay {id}...").dimmed()
    );
    let include_profile = targets.len() > 1;
    let id = id.to_string();
    let per_profile = fan_out(targets, |target| {
        let id = id.clone();
        async move {
            let api = ConfigurationOverlaysApi::new(&target.client);
            Ok(api.get(&id, include_archived).await?)
        }
    })
    .await;

    let mut results = Vec::new();
    for (profile, mut overlay) in report_errors_and_collect_successes(per_profile)? {
        if include_profile {
            render::tag_get_result(&mut overlay, &profile);
        }
        results.push(overlay);
    }
    match output {
        OutputFormat::Json => render::render_json_auto(&results)?,
        OutputFormat::Toon => {
            let toon = toon_encode(&results)
                .map_err(|error| anyhow::anyhow!("TOON encoding failed: {error}"))?;
            println!("{toon}");
        }
        OutputFormat::Text => render::render_get_text(
            &results,
            include_profile,
            "Configuration overlay not found.",
            None::<&dyn Fn(&Value)>,
        )?,
    }
    Ok(())
}

pub async fn run_create(
    targets: &[Arc<ExecutionTarget>],
    from_file: &str,
    output: OutputFormat,
) -> Result<()> {
    let body = read_from_file(from_file)?;
    eprintln!("{}", "Creating configuration overlay...".dimmed());
    let include_profile = targets.len() > 1;
    let per_profile = fan_out(targets, |target| {
        let body = body.clone();
        async move {
            let api = ConfigurationOverlaysApi::new(&target.client);
            Ok(api.create(&body).await?)
        }
    })
    .await;
    collect_write_results(
        per_profile,
        include_profile,
        output,
        "Created",
        "configuration overlay",
    )
}

pub async fn run_update(
    targets: &[Arc<ExecutionTarget>],
    id: &str,
    update_mask: &str,
    from_file: &str,
    output: OutputFormat,
) -> Result<()> {
    let body = read_from_file(from_file)?;
    eprintln!(
        "{}",
        format!("Updating configuration overlay {id}...").dimmed()
    );
    let id = id.to_string();
    let update_mask = update_mask.to_string();
    let include_profile = targets.len() > 1;
    let per_profile = fan_out(targets, |target| {
        let body = body.clone();
        let id = id.clone();
        let update_mask = update_mask.clone();
        async move {
            let api = ConfigurationOverlaysApi::new(&target.client);
            Ok(api.update(&id, &update_mask, &body).await?)
        }
    })
    .await;
    collect_write_results(
        per_profile,
        include_profile,
        output,
        "Updated",
        "configuration overlay",
    )
}

fn collect_write_results(
    per_profile: Vec<(String, Result<Value>)>,
    include_profile: bool,
    output: OutputFormat,
    verb: &str,
    kind: &str,
) -> Result<()> {
    let mut results = Vec::new();
    for (profile, mut value) in report_errors_and_collect_successes(per_profile)? {
        let name = value.get("name").and_then(Value::as_str);
        let id = value.get("id").and_then(Value::as_str);
        render::print_created(verb, kind, name, id, &profile);
        tag_profile(&mut value, include_profile, &profile);
        results.push(value);
    }
    match output {
        OutputFormat::Json => render::render_json_auto(&results)?,
        OutputFormat::Toon => {
            let toon = toon_encode(&results)
                .map_err(|error| anyhow::anyhow!("TOON encoding failed: {error}"))?;
            println!("{toon}");
        }
        OutputFormat::Text => {}
    }
    Ok(())
}

pub async fn run_archive(targets: &[Arc<ExecutionTarget>], id: &str) -> Result<()> {
    eprintln!(
        "{}",
        format!("Archiving configuration overlay {id}...").dimmed()
    );
    let id = id.to_string();
    let per_profile = fan_out(targets, |target| {
        let id = id.clone();
        async move {
            let api = ConfigurationOverlaysApi::new(&target.client);
            api.archive(&id).await?;
            Ok(())
        }
    })
    .await;
    for (profile, ()) in report_errors_and_collect_successes(per_profile)? {
        eprintln!(
            "{}",
            format!("Configuration overlay archived in profile '{profile}'.").green()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{next_page_command, validate_page_token_for_profiles};

    #[test]
    fn continuation_command_keeps_profile_and_page_options() {
        assert_eq!(
            next_page_command("staging profile", true, Some(25), "opaque-token"),
            "cx --profile 'staging profile' configuration-overlays list --include-archived --page-size 25 --page-token 'opaque-token'"
        );
    }

    #[test]
    fn page_token_is_rejected_for_multi_profile_lists() {
        let error = validate_page_token_for_profiles(2, Some("profile-a-token")).unwrap_err();
        assert!(error.to_string().contains("profile-specific"));
        assert!(validate_page_token_for_profiles(1, Some("token")).is_ok());
        assert!(validate_page_token_for_profiles(2, None).is_ok());
    }
}
