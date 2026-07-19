pub mod api;

use std::sync::Arc;

use anyhow::Result;
use colored::Colorize;
use serde_json::{json, Value};
use toon_format::encode_default as toon_encode;

use crate::config::OutputFormat;
use crate::execution::{collect_successes, fan_out, ExecutionTarget};
use crate::render;
use api::SamlApi;

fn read_from_file(path: &str) -> Result<Value> {
    let raw = if path == "-" {
        eprintln!("{}", "Reading SAML definition from stdin...".dimmed());
        use std::io::Read;
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        buf
    } else {
        eprintln!(
            "{}",
            format!("Reading SAML definition from {path}...").dimmed()
        );
        std::fs::read_to_string(path)?
    };
    Ok(serde_json::from_str(&raw)?)
}

pub async fn run_get(targets: &[Arc<ExecutionTarget>], output: OutputFormat) -> Result<()> {
    eprintln!("{}", "Fetching SAML configuration...".dimmed());
    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |t| async move {
        let api = SamlApi::new(&t.client);
        let resp = api.get_config().await?;
        Ok(serde_json::to_value(json!({
            "team_id": resp.team_id,
            "idp_details": resp.idp_details,
            "idp_parameters": resp.idp_parameters,
            "sp_parameters": resp.sp_parameters,
        }))?)
    })
    .await;

    let mut all_results: Vec<Value> = Vec::new();
    for (profile, mut val) in collect_successes(per_profile)? {
        if include_profile {
            render::tag_get_result(&mut val, &profile);
        }
        all_results.push(val);
    }

    match output {
        OutputFormat::Json => render::render_json_auto(&all_results)?,
        OutputFormat::Agents => {
            let toon = toon_encode(&all_results)
                .map_err(|e| anyhow::anyhow!("TOON encoding failed: {e}"))?;
            println!("{toon}");
        }
        OutputFormat::Text => {
            render::render_get_text(
                &all_results,
                include_profile,
                "No SAML configuration found.",
                None::<&dyn Fn(&Value)>,
            )?;
        }
    }
    Ok(())
}

pub async fn run_sp_params(targets: &[Arc<ExecutionTarget>], output: OutputFormat) -> Result<()> {
    eprintln!("{}", "Fetching SAML SP parameters...".dimmed());
    let include_profile = targets.len() > 1;

    let per_profile = fan_out(targets, |t| async move {
        let api = SamlApi::new(&t.client);
        let resp = api.get_sp_params().await?;
        Ok(resp.params.unwrap_or_else(|| json!({})))
    })
    .await;

    let mut all_results: Vec<Value> = Vec::new();
    for (profile, mut val) in collect_successes(per_profile)? {
        if include_profile {
            render::tag_get_result(&mut val, &profile);
        }
        all_results.push(val);
    }

    match output {
        OutputFormat::Json => render::render_json_auto(&all_results)?,
        OutputFormat::Agents => {
            let toon = toon_encode(&all_results)
                .map_err(|e| anyhow::anyhow!("TOON encoding failed: {e}"))?;
            println!("{toon}");
        }
        OutputFormat::Text => {
            render::render_get_text(
                &all_results,
                include_profile,
                "No SP parameters found.",
                None::<&dyn Fn(&Value)>,
            )?;
        }
    }
    Ok(())
}

pub async fn run_set_idp(
    targets: &[Arc<ExecutionTarget>],
    from_file: &str,
    output: OutputFormat,
) -> Result<()> {
    let body = read_from_file(from_file)?;
    eprintln!("{}", "Setting SAML IDP parameters...".dimmed());

    let per_profile = fan_out(targets, |t| {
        let body = body.clone();
        async move {
            let api = SamlApi::new(&t.client);
            Ok(api.set_idp_params(&body).await?)
        }
    })
    .await;

    let mut all_results: Vec<Value> = Vec::new();
    for (profile, val) in collect_successes(per_profile)? {
        eprintln!(
            "{}",
            format!("Set SAML IDP parameters in profile '{profile}'.").green()
        );
        all_results.push(val);
    }

    match output {
        OutputFormat::Json => render::render_json_auto(&all_results)?,
        OutputFormat::Agents => {
            let toon = toon_encode(&all_results)
                .map_err(|e| anyhow::anyhow!("TOON encoding failed: {e}"))?;
            println!("{toon}");
        }
        OutputFormat::Text => {}
    }
    Ok(())
}

pub async fn run_set_active(targets: &[Arc<ExecutionTarget>], active: bool) -> Result<()> {
    let action = if active { "Activating" } else { "Deactivating" };
    eprintln!("{}", format!("{action} SAML...").dimmed());
    let body = json!({ "active": active });

    let per_profile = fan_out(targets, |t| {
        let body = body.clone();
        async move {
            let api = SamlApi::new(&t.client);
            api.set_active(&body).await?;
            Ok(())
        }
    })
    .await;
    for (profile, ()) in collect_successes(per_profile)? {
        let status = if active { "activated" } else { "deactivated" };
        eprintln!(
            "{}",
            format!("SAML {status} in profile '{profile}'.").green()
        );
    }
    Ok(())
}
