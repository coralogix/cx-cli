use anyhow::{bail, Result};
use serde::Serialize;

use super::api::{GetResourcesResponse, BASE_PATH};
use crate::api_client::CxClient;
use crate::error::Result as ApiResult;

/// Scope filter keys accepted by the infrastructure resources API. Validated
/// client-side so a typo fails fast instead of round-tripping for a 400.
const ALLOWED_SCOPE_KEYS: [&str; 3] = ["service", "environment", "team"];

pub struct LegacyListParams<'p> {
    pub category: &'p str,
    pub resource_type: &'p str,
    pub name_filter: Option<&'p str>,
    pub scope_filters: &'p [(String, String)],
    pub start_row: Option<i64>,
    pub end_row: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LegacyListBody<'p> {
    category: &'p str,
    #[serde(rename = "type")]
    resource_type: &'p str,
    #[serde(skip_serializing_if = "Option::is_none")]
    name_filter: Option<&'p str>,
}

pub async fn list(
    client: &CxClient,
    params: &LegacyListParams<'_>,
) -> ApiResult<GetResourcesResponse> {
    let mut query: Vec<(String, String)> = Vec::new();
    for (key, value) in params.scope_filters {
        query.push((format!("scopeFilter.{key}"), value.clone()));
    }
    if let Some(start) = params.start_row {
        query.push(("startRow".to_string(), start.to_string()));
    }
    if let Some(end) = params.end_row {
        query.push(("endRow".to_string(), end.to_string()));
    }
    let query_refs: Vec<(&str, &str)> = query
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();

    let body = serde_json::to_value(LegacyListBody {
        category: params.category,
        resource_type: params.resource_type,
        name_filter: params.name_filter,
    })?;
    client.post_with_query(BASE_PATH, &query_refs, &body).await
}

/// Parses repeatable `--scope key=value` flags and validates keys against
/// [`ALLOWED_SCOPE_KEYS`].
///
/// Distinct keys are combined by the API with AND ("when more than one field is
/// set, a resource must match all of them"). A key given twice is rejected: each
/// scope field holds a single value server-side, so repeating one cannot express
/// "either value".
/// Failing here makes that intent explicit instead of quietly answering a
/// different question.
pub fn parse_scope_filters(scope: &[String]) -> Result<Vec<(String, String)>> {
    let mut filters: Vec<(String, String)> = Vec::new();

    for raw in scope {
        let Some((key, value)) = raw.split_once('=') else {
            bail!("invalid --scope '{raw}': expected key=value");
        };
        let key = key.trim();
        let value = value.trim();
        if !ALLOWED_SCOPE_KEYS.contains(&key) {
            bail!(
                "unknown --scope key '{key}'; allowed keys: {}",
                ALLOWED_SCOPE_KEYS.join(", ")
            );
        }
        if value.is_empty() {
            bail!("invalid --scope '{raw}': value must not be empty");
        }
        if let Some((_, existing)) = filters.iter().find(|(k, _)| k == key) {
            bail!(
                "--scope key '{key}' given more than once ('{existing}' then '{value}'); \
                 each scope key accepts a single value and different keys combine with AND, \
                 so repeating one cannot match either value - run one query per value"
            );
        }
        filters.push((key.to_string(), value.to_string()));
    }

    Ok(filters)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_scope_filters_accepts_allowed_keys() {
        let scope = vec![
            "service=checkout".to_string(),
            "environment=prod".to_string(),
            "team=platform".to_string(),
        ];
        let filters = parse_scope_filters(&scope).unwrap();
        assert_eq!(
            filters,
            vec![
                ("service".to_string(), "checkout".to_string()),
                ("environment".to_string(), "prod".to_string()),
                ("team".to_string(), "platform".to_string()),
            ]
        );
    }

    #[test]
    fn parse_scope_filters_trims_whitespace() {
        let scope = vec![" service = checkout ".to_string()];
        let filters = parse_scope_filters(&scope).unwrap();
        assert_eq!(
            filters,
            vec![("service".to_string(), "checkout".to_string())]
        );
    }

    #[test]
    fn parse_scope_filters_rejects_missing_equals() {
        let err = parse_scope_filters(&["service".to_string()]).unwrap_err();
        assert!(err.to_string().contains("expected key=value"));
    }

    #[test]
    fn parse_scope_filters_rejects_unknown_key() {
        let err = parse_scope_filters(&["region=us-east-1".to_string()]).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("unknown --scope key 'region'"));
        assert!(msg.contains("service, environment, team"));
    }

    #[test]
    fn parse_scope_filters_rejects_empty_value() {
        let err = parse_scope_filters(&["service=".to_string()]).unwrap_err();
        assert!(err.to_string().contains("must not be empty"));
    }

    #[test]
    fn parse_scope_filters_empty_input_yields_no_filters() {
        assert!(parse_scope_filters(&[]).unwrap().is_empty());
    }

    #[test]
    fn parse_scope_filters_rejects_a_repeated_key() {
        let err =
            parse_scope_filters(&["service=a".to_string(), "service=b".to_string()]).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("'service' given more than once"), "got: {msg}");
        assert!(msg.contains('a') && msg.contains('b'), "got: {msg}");
    }

    #[test]
    fn parse_scope_filters_rejects_a_repeated_key_even_with_the_same_value() {
        let err =
            parse_scope_filters(&["service=a".to_string(), "service=a".to_string()]).unwrap_err();
        assert!(err.to_string().contains("given more than once"));
    }

    #[test]
    fn parse_scope_filters_detects_a_repeat_after_trimming() {
        let err = parse_scope_filters(&[" service = a ".to_string(), "service=b".to_string()])
            .unwrap_err();
        assert!(err.to_string().contains("given more than once"));
    }

    #[test]
    fn parse_scope_filters_allows_distinct_keys_sharing_a_value() {
        let filters =
            parse_scope_filters(&["service=core".to_string(), "team=core".to_string()]).unwrap();
        assert_eq!(
            filters,
            vec![
                ("service".to_string(), "core".to_string()),
                ("team".to_string(), "core".to_string()),
            ]
        );
    }
}
