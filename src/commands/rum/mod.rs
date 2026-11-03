pub mod api;

use std::sync::Arc;

use anyhow::{bail, Result};
use colored::Colorize;
use serde_json::{json, Value};

use crate::config::OutputFormat;
use crate::execution::{fan_out, report_errors_and_collect_successes, ExecutionTarget};
use crate::render;
use api::{short_enum, MapStackTraceResponse, RumApi, SourceMapRelease, StackFrame};

const PLATFORM_PREFIX: &str = "SOURCE_MAP_PLATFORM_";
const STATUS_PREFIX: &str = "STACK_FRAME_MAPPING_STATUS_";

/// Maps the `--platform` flag to the API enum.
pub fn platform_enum(platform: &str) -> Result<&'static str> {
    match platform.trim().to_lowercase().replace('_', "-").as_str() {
        "web" => Ok("SOURCE_MAP_PLATFORM_WEB"),
        "android" | "react-native-android" => Ok("SOURCE_MAP_PLATFORM_REACT_NATIVE_ANDROID"),
        "ios" | "react-native-ios" => Ok("SOURCE_MAP_PLATFORM_REACT_NATIVE_IOS"),
        _ => bail!("unknown platform '{platform}'; allowed: web, android, ios"),
    }
}

fn read_input(path: &str) -> Result<String> {
    if path == "-" {
        eprintln!("{}", "Reading stack frames from stdin...".dimmed());
        let mut buf = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf)?;
        Ok(buf)
    } else {
        Ok(std::fs::read_to_string(path)?)
    }
}

/// Collects frames from `--frame` flags, or else from the `--from-file` JSON (`-` for stdin).
pub fn collect_frames(frame_args: &[String], from_file: Option<&str>) -> Result<Vec<StackFrame>> {
    let frames = if frame_args.is_empty() {
        // Never read stdin implicitly: a terminal or an agent without stdin would wait forever.
        let Some(path) = from_file else {
            bail!("no stack frames given; pass --frame <file>:<line>[:<column>] or --from-file <path|->");
        };
        let (frames, skipped) =
            api::parse_frames(&read_input(path)?).map_err(anyhow::Error::msg)?;
        if skipped > 0 {
            eprintln!(
                "{}",
                format!("Skipped {skipped} frames without a line number (native, eval or anonymous code).")
                    .yellow()
            );
        }
        frames
    } else {
        frame_args
            .iter()
            .map(|f| api::parse_frame_arg(f))
            .collect::<std::result::Result<_, _>>()
            .map_err(anyhow::Error::msg)?
    };
    if frames.is_empty() {
        bail!("no mappable stack frames: every frame lacks a line number");
    }
    Ok(frames)
}

/// List rows carry `profile`, like every other list command (get-style results use `_profile`).
fn tag_list_row(mut value: Value, include_profile: bool, profile: &str) -> Value {
    if let (true, Value::Object(map)) = (include_profile, &mut value) {
        map.insert("profile".to_string(), Value::String(profile.to_string()));
    }
    value
}

// ── Subcommand runners ────────────────────────────────────────────────────────

pub async fn run_source_maps_list(
    targets: &[Arc<ExecutionTarget>],
    application: Option<&str>,
    output: OutputFormat,
) -> Result<()> {
    eprintln!("{}", "Fetching source map releases...".dimmed());

    let include_profile = targets.len() > 1;
    let application = application.map(str::to_string);

    let per_profile = fan_out(targets, |t| {
        let application = application.clone();
        async move {
            let api = RumApi::new(&t.client);
            Ok(api.list_releases(application.as_deref()).await?)
        }
    })
    .await;

    let mut all: Vec<(String, SourceMapRelease)> = Vec::new();
    for (profile, releases) in report_errors_and_collect_successes(per_profile)? {
        all.extend(releases.into_iter().map(|r| (profile.clone(), r)));
    }

    let as_json = || -> Vec<Value> {
        all.iter()
            .map(|(profile, r)| tag_list_row(json!(r), include_profile, profile))
            .collect()
    };
    match output {
        OutputFormat::Json => render::render_json(&as_json())?,
        OutputFormat::Toon => render::render_toon(&as_json())?,
        OutputFormat::Text => {
            if all.is_empty() {
                render::print_no_results("No source map releases found.");
                return Ok(());
            }
            let rows = all
                .iter()
                .map(|(profile, r)| {
                    vec![
                        profile.clone(),
                        r.application.clone(),
                        r.version.clone(),
                        short_enum(r.platform.as_deref(), PLATFORM_PREFIX),
                        short_enum(r.upload_status.as_deref(), "SOURCE_MAP_UPLOAD_STATUS_"),
                        r.commit_hash.clone().unwrap_or_default(),
                        r.create_time.clone().unwrap_or_default(),
                    ]
                })
                .collect();
            render::render_table(
                &[
                    "Application",
                    "Version",
                    "Platform",
                    "Upload",
                    "Commit",
                    "Uploaded",
                ],
                rows,
                include_profile,
            );
        }
    }
    Ok(())
}

pub async fn run_stacktrace(
    targets: &[Arc<ExecutionTarget>],
    application: &str,
    version: &str,
    platform: &str,
    frames: Vec<StackFrame>,
    output: OutputFormat,
) -> Result<()> {
    let platform = platform_enum(platform)?;
    eprintln!(
        "{}",
        format!(
            "Mapping {} frames for {application} {version}...",
            frames.len()
        )
        .dimmed()
    );

    let include_profile = targets.len() > 1;
    let (application, version, frames) = (
        application.to_string(),
        version.to_string(),
        Arc::new(frames),
    );

    let per_profile = fan_out(targets, |t| {
        let (application, version, frames) = (application.clone(), version.clone(), frames.clone());
        async move {
            let api = RumApi::new(&t.client);
            Ok(api
                .map_stack_trace(&application, &version, platform, &frames)
                .await?)
        }
    })
    .await;
    let results = report_errors_and_collect_successes(per_profile)?;

    let as_json = || -> Vec<Value> {
        results
            .iter()
            .map(|(profile, r)| {
                let mut value = json!(r);
                if include_profile {
                    render::tag_get_result(&mut value, profile);
                }
                value
            })
            .collect()
    };
    match output {
        OutputFormat::Json => render::render_json_auto(&as_json())?,
        OutputFormat::Toon => render::render_toon(&as_json())?,
        OutputFormat::Text => {
            for (profile, response) in &results {
                if include_profile {
                    println!("{}", format!("[{profile}]").bold());
                }
                print_mapped_text(response);
            }
        }
    }
    Ok(())
}

fn print_mapped_text(response: &MapStackTraceResponse) {
    let commit = match (
        response.commit_hash.as_deref(),
        response.repo_name.as_deref(),
    ) {
        (Some(c), Some(r)) if !c.is_empty() && !r.is_empty() => format!("  commit {c} ({r})"),
        (Some(c), _) if !c.is_empty() => format!("  commit {c}"),
        _ => String::new(),
    };
    println!(
        "{} {}{}",
        response.application.bold(),
        response.version,
        commit.dimmed()
    );

    for (index, frame) in response.frames.iter().enumerate() {
        let minified = match frame.column_number {
            Some(col) => format!("{}:{}:{col}", frame.file_name, frame.line_number),
            None => format!("{}:{}", frame.file_name, frame.line_number),
        };
        let Some(source) = &frame.source else {
            let status = short_enum(frame.status.as_deref(), STATUS_PREFIX)
                .to_lowercase()
                .replace('_', " ");
            println!("#{index} {minified}  {}", format!("[{status}]").yellow());
            continue;
        };
        let function = if source.function_name.is_empty() {
            "<anonymous>"
        } else {
            &source.function_name
        };
        println!(
            "#{index} {} {}",
            function.bold(),
            format!(
                "{}:{}:{}",
                source.file_name, source.line_number, source.column_number
            )
            .green()
        );
        for line in &source.code_lines {
            let marker = if line.line_number == source.line_number {
                ">"
            } else {
                " "
            };
            let focus = (marker == ">").then_some(source.column_number as usize);
            println!(
                "   {marker} {:>5} | {}",
                line.line_number,
                clip_line(&line.text, focus)
            );
        }
    }
}

const MAX_TEXT_LINE: usize = 160;

/// Keeps text output readable for minified sources: a long line shows a window around
/// `focus` (the mapped column) when given, else its start. JSON output keeps the full line.
fn clip_line(text: &str, focus: Option<usize>) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= MAX_TEXT_LINE {
        return text.to_string();
    }
    let start = focus
        .map(|col| col.saturating_sub(MAX_TEXT_LINE / 3))
        .unwrap_or(0)
        .min(chars.len() - MAX_TEXT_LINE);
    let end = start + MAX_TEXT_LINE;
    let mut clipped: String = chars[start..end].iter().collect();
    if start > 0 {
        clipped.insert(0, '…');
    }
    if end < chars.len() {
        clipped.push('…');
    }
    clipped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_rows_only_for_multiple_profiles() {
        assert_eq!(
            tag_list_row(json!({"v": 1}), true, "eu2"),
            json!({"v": 1, "profile": "eu2"})
        );
        assert_eq!(tag_list_row(json!({"v": 1}), false, "eu2"), json!({"v": 1}));
    }

    #[test]
    fn refuses_to_read_stdin_implicitly() {
        let error = collect_frames(&[], None).unwrap_err().to_string();
        assert!(error.contains("--from-file"), "{error}");
    }

    #[test]
    fn clips_long_lines_around_the_mapped_column() {
        assert_eq!(clip_line("short", Some(2)), "short");

        let long: String = (0..1000)
            .map(|i| char::from(b'a' + (i % 26) as u8))
            .collect();
        let head = clip_line(&long, None);
        assert!(head.starts_with("abc") && head.ends_with('…'));
        assert_eq!(head.chars().count(), MAX_TEXT_LINE + 1);

        let around = clip_line(&long, Some(500));
        assert!(around.starts_with('…') && around.ends_with('…'));
        assert!(around.contains(&long[495..505]));

        let tail = clip_line(&long, Some(999));
        assert!(tail.starts_with('…') && tail.ends_with(&long[990..]));
    }

    #[test]
    fn maps_platform_flag() {
        assert_eq!(platform_enum("web").unwrap(), "SOURCE_MAP_PLATFORM_WEB");
        assert_eq!(
            platform_enum("Android").unwrap(),
            "SOURCE_MAP_PLATFORM_REACT_NATIVE_ANDROID"
        );
        assert_eq!(
            platform_enum("react_native_ios").unwrap(),
            "SOURCE_MAP_PLATFORM_REACT_NATIVE_IOS"
        );
        assert!(platform_enum("windows").is_err());
    }

    #[test]
    fn frame_flags_win_over_file() {
        let frames = collect_frames(&["main.js:1:2".into()], Some("/does/not/exist")).unwrap();
        assert_eq!(frames.len(), 1);
    }
}
