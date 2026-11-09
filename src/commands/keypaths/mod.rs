use std::sync::Arc;

use anyhow::{bail, Result};
use colored::Colorize;
use serde::Serialize;
use serde_json::Value;

pub mod api;

use crate::commands::dashboards::profiled_api_row_to_json;
use crate::config::OutputFormat;
use crate::execution::{fan_out, report_errors_and_collect_successes, ExecutionTarget};
use crate::render;
use crate::time::parse_timestamp_nanos;
use api::{
    DimensionMatch, GetKeypathSamplesRequest, Keypath, KeypathsApi, ListKeypathsRequest,
    NamedDataset, TimeRange,
};

const DEFAULT_DATASPACE: &str = "default";

/// `dataspace/dataset`, or a bare `dataset` in the `default` dataspace.
pub fn parse_dataset(input: &str) -> Result<NamedDataset> {
    let (dataspace, dataset) = match input.split_once('/') {
        Some((dataspace, dataset)) => (dataspace, dataset),
        None => (DEFAULT_DATASPACE, input),
    };
    if dataspace.is_empty() || dataset.is_empty() || dataset.contains('/') {
        bail!("invalid dataset '{input}': expected 'dataset' or 'dataspace/dataset'");
    }
    Ok(NamedDataset {
        dataspace: dataspace.to_string(),
        dataset: dataset.to_string(),
    })
}

/// `--label name=value` flags as label matches: `name` may also be written `$l.name`, and repeating
/// a name adds a value it may match.
pub fn parse_labels(inputs: &[String]) -> Result<Vec<DimensionMatch>> {
    let mut matches: Vec<DimensionMatch> = Vec::new();
    for input in inputs {
        let Some((name, value)) = input.split_once('=') else {
            bail!(
                "invalid --label '{input}': expected 'name=value', e.g. 'applicationName=checkout'"
            );
        };
        let name = name.trim();
        let name = name.strip_prefix("$l.").unwrap_or(name);
        if name.is_empty() {
            bail!("invalid --label '{input}': the label name is empty");
        }
        let keypath = format!("$l.{name}");
        match matches.iter_mut().find(|m| m.keypath == keypath) {
            Some(existing) => existing.values.push(value.to_string()),
            None => matches.push(DimensionMatch {
                keypath,
                values: vec![value.to_string()],
            }),
        }
    }
    Ok(matches)
}

pub fn parse_datasets(inputs: &[String]) -> Result<Vec<NamedDataset>> {
    inputs.iter().map(|d| parse_dataset(d)).collect()
}

pub fn time_range(start: &str, end: &str) -> Result<TimeRange> {
    Ok(TimeRange {
        start: parse_timestamp_nanos(start)?,
        end: parse_timestamp_nanos(end)?,
    })
}

pub async fn run_list(
    targets: &[Arc<ExecutionTarget>],
    request: &ListKeypathsRequest,
    output: OutputFormat,
) -> Result<()> {
    eprintln!("{}", "Listing keypaths…".dimmed());

    let include_profile = targets.len() > 1;
    let per_profile = fan_out(targets, |target| {
        let request = request.clone();
        async move {
            KeypathsApi::new(&target.client)
                .list(&request)
                .await
                .map_err(Into::into)
        }
    })
    .await;

    let mut keypaths: Vec<(String, Keypath)> = Vec::new();
    let mut truncated = false;
    for (profile, resp) in report_errors_and_collect_successes(per_profile)? {
        truncated |= resp.truncated;
        keypaths.extend(resp.keypaths.into_iter().map(|k| (profile.clone(), k)));
    }
    keypaths.sort_by(|(pa, a), (pb, b)| (pa, &a.keypath).cmp(&(pb, &b.keypath)));

    if truncated {
        eprintln!(
            "{}",
            "More keypaths matched than --limit. Narrow with --contains, --prefix, --scope or --label, or raise --limit."
                .yellow()
        );
    }

    match output {
        OutputFormat::Json | OutputFormat::Toon => {
            let rows = keypaths
                .iter()
                .map(|(profile, k)| {
                    profiled_api_row_to_json(profile, k, include_profile, "keypath")
                })
                .collect::<Result<Vec<Value>>>()?;
            if output == OutputFormat::Json {
                render::render_json(&rows)?;
            } else {
                render::render_toon(&rows)?;
            }
        }
        OutputFormat::Text => {
            if keypaths.is_empty() {
                render::print_no_results("No keypaths found.");
                return Ok(());
            }
            let rows = keypaths
                .iter()
                .map(|(profile, k)| vec![profile.clone(), k.keypath.clone(), k.types.join(", ")])
                .collect();
            render::render_table(&["Keypath", "Types"], rows, include_profile);
        }
    }
    Ok(())
}

#[derive(Serialize)]
struct SampleRow<'a> {
    sample: &'a str,
}

pub async fn run_samples(
    targets: &[Arc<ExecutionTarget>],
    request: &GetKeypathSamplesRequest,
    output: OutputFormat,
) -> Result<()> {
    eprintln!(
        "{}",
        format!("Sampling values of {}…", request.keypath).dimmed()
    );

    let include_profile = targets.len() > 1;
    let per_profile = fan_out(targets, |target| {
        let request = request.clone();
        async move {
            KeypathsApi::new(&target.client)
                .samples(&request)
                .await
                .map_err(Into::into)
        }
    })
    .await;

    let mut samples: Vec<(String, String)> = Vec::new();
    for (profile, resp) in report_errors_and_collect_successes(per_profile)? {
        samples.extend(resp.samples.into_iter().map(|s| (profile.clone(), s)));
    }
    samples.sort();

    match output {
        OutputFormat::Json | OutputFormat::Toon => {
            let rows = samples
                .iter()
                .map(|(profile, sample)| {
                    profiled_api_row_to_json(
                        profile,
                        &SampleRow { sample },
                        include_profile,
                        "sample",
                    )
                })
                .collect::<Result<Vec<Value>>>()?;
            if output == OutputFormat::Json {
                render::render_json(&rows)?;
            } else {
                render::render_toon(&rows)?;
            }
        }
        OutputFormat::Text => {
            if samples.is_empty() {
                render::print_no_results("No samples found.");
                return Ok(());
            }
            let rows = samples
                .iter()
                .map(|(profile, sample)| vec![profile.clone(), sample.clone()])
                .collect();
            render::render_table(&["Sample"], rows, include_profile);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_dataset_is_in_the_default_dataspace() {
        assert_eq!(
            parse_dataset("logs").unwrap(),
            NamedDataset {
                dataspace: "default".into(),
                dataset: "logs".into()
            }
        );
    }

    #[test]
    fn qualified_dataset_keeps_its_dataspace() {
        assert_eq!(
            parse_dataset("system/engine.queries").unwrap(),
            NamedDataset {
                dataspace: "system".into(),
                dataset: "engine.queries".into()
            }
        );
    }

    #[test]
    fn malformed_datasets_are_rejected() {
        for bad in ["", "/logs", "default/", "a/b/c"] {
            assert!(parse_dataset(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn label_names_get_the_label_root() {
        assert_eq!(
            parse_labels(&["applicationName=a=b".into(), "$l.subsystemName=api".into()]).unwrap(),
            [
                DimensionMatch {
                    keypath: "$l.applicationName".into(),
                    values: vec!["a=b".into()]
                },
                DimensionMatch {
                    keypath: "$l.subsystemName".into(),
                    values: vec!["api".into()]
                },
            ]
        );
    }

    #[test]
    fn repeated_label_names_merge_their_values() {
        assert_eq!(
            parse_labels(&[
                "applicationName=checkout".into(),
                "$l.applicationName=payments".into()
            ])
            .unwrap(),
            [DimensionMatch {
                keypath: "$l.applicationName".into(),
                values: vec!["checkout".into(), "payments".into()]
            }]
        );
    }

    #[test]
    fn malformed_labels_are_rejected() {
        for bad in ["applicationName", "=checkout", "$l.=checkout", ""] {
            assert!(parse_labels(&[bad.into()]).is_err(), "{bad}");
        }
    }
}
