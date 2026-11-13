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
use crate::time::parse_timestamp_nanos_at;
use api::{
    DimensionFilter, DimensionFilters, DimensionMatch, GetKeypathSamplesRequest,
    KeypathSearchResult, KeypathsApi, NamedDataset, SearchKeypathsRequest, TextFilter, TextFilters,
    TimeRange,
};
use chrono::Utc;

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

/// `--label name=value` flags as a dimension filter: `name` may also be written `$l.name`,
/// repeating a name adds a value it may match, and different names must all match.
pub fn parse_labels(inputs: &[String]) -> Result<Option<DimensionFilter>> {
    let mut labels: Vec<(String, Vec<DimensionFilter>)> = Vec::new();
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
        let condition = DimensionFilter::Match(DimensionMatch {
            keypath: keypath.clone(),
            value: value.to_string(),
        });
        match labels.iter_mut().find(|(existing, _)| *existing == keypath) {
            Some((_, matches)) => matches.push(condition),
            None => labels.push((keypath, vec![condition])),
        }
    }
    let mut filters: Vec<DimensionFilter> = labels
        .into_iter()
        .map(|(_, mut matches)| {
            if matches.len() == 1 {
                matches.remove(0)
            } else {
                DimensionFilter::Any(DimensionFilters { filters: matches })
            }
        })
        .collect();
    Ok(match filters.len() {
        0 => None,
        1 => filters.pop(),
        _ => Some(DimensionFilter::All(DimensionFilters { filters })),
    })
}

/// The filter for `--exact`, `--prefix` and `--contains`, on keypaths or on values: repeating a flag
/// means any of its values, and different flags must all hold.
pub fn text_filter(
    exact: Vec<String>,
    prefixes: Vec<String>,
    contains: Vec<String>,
) -> Option<TextFilter> {
    let any_of = |values: Vec<String>, condition: fn(String) -> TextFilter| {
        let mut filters: Vec<TextFilter> = values.into_iter().map(condition).collect();
        match filters.len() {
            0 => None,
            1 => filters.pop(),
            _ => Some(TextFilter::Any(TextFilters { filters })),
        }
    };
    let mut filters: Vec<TextFilter> = [
        any_of(exact, TextFilter::Exact),
        any_of(prefixes, TextFilter::Prefix),
        any_of(contains, TextFilter::Contains),
    ]
    .into_iter()
    .flatten()
    .collect();
    match filters.len() {
        0 => None,
        1 => filters.pop(),
        _ => Some(TextFilter::All(TextFilters { filters })),
    }
}

pub fn parse_datasets(inputs: &[String]) -> Result<Vec<NamedDataset>> {
    inputs.iter().map(|d| parse_dataset(d)).collect()
}

/// Both bounds resolve against one clock reading, so `now-7d`..`now` spans exactly seven days.
pub fn time_range(start: &str, end: &str) -> Result<TimeRange> {
    let now = Utc::now();
    Ok(TimeRange {
        start: parse_timestamp_nanos_at(start, now)?,
        end: parse_timestamp_nanos_at(end, now)?,
    })
}

pub async fn run_search(
    targets: &[Arc<ExecutionTarget>],
    request: &SearchKeypathsRequest,
    output: OutputFormat,
) -> Result<()> {
    eprintln!("{}", "Searching keypaths…".dimmed());

    let include_profile = targets.len() > 1;
    let per_profile = fan_out(targets, |target| {
        let request = request.clone();
        async move {
            KeypathsApi::new(&target.client)
                .search(&request)
                .await
                .map_err(Into::into)
        }
    })
    .await;

    let mut keypaths: Vec<(String, KeypathSearchResult)> = Vec::new();
    for (profile, resp) in report_errors_and_collect_successes(per_profile)? {
        keypaths.extend(resp.keypaths.into_iter().map(|k| (profile.clone(), k)));
    }
    keypaths.sort_by(|(pa, a), (pb, b)| (pa, &a.keypath).cmp(&(pb, &b.keypath)));

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
                render::print_no_results("No fields found.");
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
struct ValueRow<'a> {
    value: &'a str,
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
                .map(|(profile, value)| {
                    profiled_api_row_to_json(profile, &ValueRow { value }, include_profile, "value")
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
                render::print_no_results("No values found.");
                return Ok(());
            }
            let rows = samples
                .iter()
                .map(|(profile, value)| vec![profile.clone(), value.clone()])
                .collect();
            render::render_table(&["Value"], rows, include_profile);
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

    fn label(keypath: &str, values: &[&str]) -> DimensionFilter {
        let mut matches: Vec<DimensionFilter> = values
            .iter()
            .map(|value| {
                DimensionFilter::Match(DimensionMatch {
                    keypath: keypath.into(),
                    value: value.to_string(),
                })
            })
            .collect();
        if matches.len() == 1 {
            matches.remove(0)
        } else {
            DimensionFilter::Any(DimensionFilters { filters: matches })
        }
    }

    #[test]
    fn relative_bounds_share_one_clock_reading() {
        let range = time_range("now-7d", "now").unwrap();
        let start = chrono::DateTime::parse_from_rfc3339(&range.start).unwrap();
        let end = chrono::DateTime::parse_from_rfc3339(&range.end).unwrap();
        assert_eq!(end - start, chrono::Duration::days(7));
    }

    #[test]
    fn no_labels_is_no_filter() {
        assert_eq!(parse_labels(&[]).unwrap(), None);
    }

    #[test]
    fn one_label_is_a_match() {
        assert_eq!(
            parse_labels(&["applicationName=a=b".into()]).unwrap(),
            Some(label("$l.applicationName", &["a=b"]))
        );
    }

    #[test]
    fn different_labels_must_all_match() {
        assert_eq!(
            parse_labels(&[
                "applicationName=checkout".into(),
                "$l.subsystemName=api".into()
            ])
            .unwrap(),
            Some(DimensionFilter::All(DimensionFilters {
                filters: vec![
                    label("$l.applicationName", &["checkout"]),
                    label("$l.subsystemName", &["api"]),
                ]
            }))
        );
    }

    #[test]
    fn a_repeated_label_name_matches_any_of_its_values() {
        assert_eq!(
            parse_labels(&[
                "applicationName=checkout".into(),
                "$l.applicationName=payments".into()
            ])
            .unwrap(),
            Some(label("$l.applicationName", &["checkout", "payments"]))
        );
    }

    #[test]
    fn malformed_labels_are_rejected() {
        for bad in ["applicationName", "=checkout", "$l.=checkout", ""] {
            assert!(parse_labels(&[bad.into()]).is_err(), "{bad}");
        }
    }

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| v.to_string()).collect()
    }

    #[test]
    fn no_keypath_flags_is_no_filter() {
        assert_eq!(text_filter(vec![], vec![], vec![]), None);
    }

    #[test]
    fn one_keypath_flag_is_one_condition() {
        assert_eq!(
            text_filter(vec![], strings(&["$d"]), vec![]),
            Some(TextFilter::Prefix("$d".into()))
        );
    }

    #[test]
    fn repeated_flags_mean_any_and_different_flags_must_all_hold() {
        assert_eq!(
            text_filter(
                vec![],
                strings(&["$d.kubernetes", "$d.k8s"]),
                strings(&["pod"])
            ),
            Some(TextFilter::All(TextFilters {
                filters: vec![
                    TextFilter::Any(TextFilters {
                        filters: vec![
                            TextFilter::Prefix("$d.kubernetes".into()),
                            TextFilter::Prefix("$d.k8s".into()),
                        ]
                    }),
                    TextFilter::Contains("pod".into()),
                ]
            }))
        );
    }
}
