pub mod api;

use std::sync::Arc;

use anyhow::Result;
use colored::Colorize;
use serde_json::{json, Map, Value};
use toon_format::encode_default as toon_encode;

use crate::config::OutputFormat;
use crate::execution::{fan_out, report_errors_and_collect_successes, ExecutionTarget};
use crate::render;
use api::ConfigurationGroupsApi;

fn read_from_file(path: &str) -> Result<Value> {
    let raw = if path == "-" {
        eprintln!(
            "{}",
            "Reading configuration group definition from stdin...".dimmed()
        );
        use std::io::Read;
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        buf
    } else {
        eprintln!(
            "{}",
            format!("Reading configuration group definition from {path}...").dimmed()
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

fn group_row(group: &Value, profile: &str) -> Vec<String> {
    vec![
        profile.to_string(),
        field_text(group.get("id")),
        field_text(group.get("name")),
        field_text(group.get("priorityOrder")),
        field_text(group.pointer("/family/active")),
        field_text(group.get("archived")),
        field_text(group.pointer("/family/version")),
    ]
}

fn tag_profile(group: &mut Value, include_profile: bool, profile: &str) {
    if include_profile {
        if let Value::Object(object) = group {
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
        "configuration-groups".to_string(),
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

pub async fn run_list(
    targets: &[Arc<ExecutionTarget>],
    include_archived: bool,
    page_size: Option<u16>,
    page_token: Option<&str>,
    output: OutputFormat,
) -> Result<()> {
    eprintln!("{}", "Fetching configuration groups...".dimmed());
    let include_profile = targets.len() > 1;
    let page_token = page_token.map(str::to_string);
    let per_profile = fan_out(targets, |target| {
        let page_token = page_token.clone();
        async move {
            let api = ConfigurationGroupsApi::new(&target.client);
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
        for mut group in response.groups {
            tag_profile(&mut group, include_profile, &profile);
            all_rows.push(group_row(&group, &profile));
            all_json.push(group);
        }
    }

    if !next_page_tokens.is_empty() {
        eprintln!(
            "{}",
            "More configuration groups are available. Continue with --page-token <token>, reusing the same --include-archived and --page-size options; next-page tokens are included in JSON and TOON output."
                .yellow()
        );
    }

    match output {
        OutputFormat::Json | OutputFormat::Toon => {
            let mut response = json!({ "groups": all_json });
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
                render::print_no_results("No configuration groups found.");
                return Ok(());
            }
            render::render_table(
                &["ID", "Name", "Priority", "Active", "Archived", "Version"],
                all_rows,
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
        format!("Fetching configuration group {id}...").dimmed()
    );
    let include_profile = targets.len() > 1;
    let id = id.to_string();
    let per_profile = fan_out(targets, |target| {
        let id = id.clone();
        async move {
            let api = ConfigurationGroupsApi::new(&target.client);
            Ok(api.get(&id, include_archived).await?)
        }
    })
    .await;

    let mut results = Vec::new();
    for (profile, mut group) in report_errors_and_collect_successes(per_profile)? {
        tag_profile(&mut group, include_profile, &profile);
        results.push(group);
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
            "Configuration group not found.",
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
    eprintln!("{}", "Creating configuration group...".dimmed());
    let include_profile = targets.len() > 1;
    let per_profile = fan_out(targets, |target| {
        let body = body.clone();
        async move {
            let api = ConfigurationGroupsApi::new(&target.client);
            Ok(api.create(&body).await?)
        }
    })
    .await;
    collect_write_results(
        per_profile,
        include_profile,
        output,
        "Created configuration group",
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
        format!("Updating configuration group {id}...").dimmed()
    );
    let id = id.to_string();
    let update_mask = update_mask.to_string();
    let include_profile = targets.len() > 1;
    let per_profile = fan_out(targets, |target| {
        let body = body.clone();
        let id = id.clone();
        let update_mask = update_mask.clone();
        async move {
            let api = ConfigurationGroupsApi::new(&target.client);
            Ok(api.update(&id, &update_mask, &body).await?)
        }
    })
    .await;
    collect_write_results(
        per_profile,
        include_profile,
        output,
        "Updated configuration group",
    )
}

fn collect_write_results(
    per_profile: Vec<(String, Result<Value>)>,
    include_profile: bool,
    output: OutputFormat,
    success: &str,
) -> Result<()> {
    let mut results = Vec::new();
    for (profile, mut value) in report_errors_and_collect_successes(per_profile)? {
        eprintln!("{}", format!("{success} in profile '{profile}'.").green());
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
        format!("Archiving configuration group {id}...").dimmed()
    );
    let id = id.to_string();
    let per_profile = fan_out(targets, |target| {
        let id = id.clone();
        async move {
            let api = ConfigurationGroupsApi::new(&target.client);
            api.archive(&id).await?;
            Ok(())
        }
    })
    .await;
    for (profile, ()) in report_errors_and_collect_successes(per_profile)? {
        eprintln!(
            "{}",
            format!("Configuration group archived in profile '{profile}'.").green()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::next_page_command;

    #[test]
    fn continuation_command_keeps_profile_and_page_options() {
        assert_eq!(
            next_page_command("staging profile", true, Some(25), "opaque-token"),
            "cx --profile 'staging profile' configuration-groups list --include-archived --page-size 25 --page-token 'opaque-token'"
        );
    }
}
