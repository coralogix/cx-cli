use anyhow::bail;
use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use serde::Deserialize;
use serde_json::{Map, Value};

use crate::api_client::CxClient;
use crate::error::{CxError, Result};
use crate::serde_helpers::string_or_number;

const BASE_PATH: &str = "/mgmt/openapi/5/product-analytics";
const APPLICATIONS_PATH: &str = "/applications/v1";
const VIEWS_PATH: &str = "/views/v1";
const EVENTS_PATH: &str = "/events/v1";
const FUNNELS_PATH: &str = "/funnels/v1";
const PATHWAYS_PATH: &str = "/pathways/v1";
const HEATMAPS_PATH: &str = "/heatmaps/v1";
const SETTINGS_PATH: &str = "/settings/v1";

const APP_NAMES_PARAM: &str = "app_names";
const PAGE_SIZE_PARAM: &str = "page_size";
const PAGE_TOKEN_PARAM: &str = "page_token";
const UPDATE_MASK_PARAM: &str = "updateMask";

const TIME_RANGE_FIELD: &str = "timeRange";
const SETTINGS_ENABLED_FIELD: &str = "enabled";
const READ_ONLY_FIELDS: &[&str] = &[
    "id",
    "appName",
    "createdAt",
    "updatedAt",
    "owner",
    "version",
];

pub const DEFAULT_TIME_RANGE_MINUTES: i64 = 24 * 60;

const ENUM_PREFIXES: &[&str] = &[
    "FUNNEL_MODE_",
    "FUNNEL_OUTCOME_",
    "PATHWAY_DIRECTION_",
    "HEATMAP_MODE_",
    "ACTION_NAME_FILTER_",
    "EVENT_SOURCE_",
];

const PRODUCT_ANALYTICS_DISABLED_HINT: &str = "Check it with `cx product-analytics settings get`; \
     a team admin can turn it on with `cx product-analytics settings update --enabled true`.";

const PATH_SEGMENT_ENCODE_SET: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');

const QUERY_VALUE_ENCODE_SET: &AsciiSet = &PATH_SEGMENT_ENCODE_SET.remove(b',');

fn encode_segment(segment: &str) -> String {
    utf8_percent_encode(segment, PATH_SEGMENT_ENCODE_SET).to_string()
}

fn explain_disabled(error: CxError) -> CxError {
    match error {
        CxError::Api {
            status: 412,
            message,
        } => {
            let message = if message.trim().is_empty() {
                "Product Analytics is not enabled for this team.".to_string()
            } else {
                message
            };
            CxError::Api {
                status: 412,
                message: format!("{message} {PRODUCT_ANALYTICS_DISABLED_HINT}"),
            }
        }
        other => other,
    }
}

pub fn display_enum(value: Option<&str>) -> String {
    let Some(value) = value else {
        return "-".to_string();
    };
    ENUM_PREFIXES
        .iter()
        .find_map(|prefix| value.strip_prefix(prefix))
        .unwrap_or(value)
        .to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum EventSource {
    Auto,
    Custom,
}

impl EventSource {
    pub fn wire(self) -> &'static str {
        match self {
            Self::Auto => "EVENT_SOURCE_AUTO",
            Self::Custom => "EVENT_SOURCE_CUSTOM",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum ActionNameFilter {
    All,
    Named,
    Unnamed,
}

impl ActionNameFilter {
    pub fn wire(self) -> &'static str {
        match self {
            Self::All => "ACTION_NAME_FILTER_ALL_CLICKS",
            Self::Named => "ACTION_NAME_FILTER_NAMED_ONLY",
            Self::Unnamed => "ACTION_NAME_FILTER_UNNAMED_ONLY",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum FunnelMode {
    Users,
    Sessions,
    Accounts,
}

impl FunnelMode {
    pub fn wire(self) -> &'static str {
        match self {
            Self::Users => "FUNNEL_MODE_USERS",
            Self::Sessions => "FUNNEL_MODE_SESSIONS",
            Self::Accounts => "FUNNEL_MODE_ACCOUNTS",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum FunnelOutcome {
    Converted,
    Dropped,
}

impl FunnelOutcome {
    pub fn wire(self) -> &'static str {
        match self {
            Self::Converted => "FUNNEL_OUTCOME_CONVERTED",
            Self::Dropped => "FUNNEL_OUTCOME_DROPPED",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum UserIdMode {
    Hashed,
    Raw,
}

impl UserIdMode {
    pub fn wire(self) -> &'static str {
        match self {
            Self::Hashed => "USER_ID_MODE_HASHED",
            Self::Raw => "USER_ID_MODE_RAW",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum PathwayDirection {
    After,
    Before,
}

impl PathwayDirection {
    pub fn wire(self) -> &'static str {
        match self {
            Self::After => "PATHWAY_DIRECTION_AFTER",
            Self::Before => "PATHWAY_DIRECTION_BEFORE",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum CalendarWindow {
    Today,
    Yesterday,
}

impl CalendarWindow {
    pub fn wire(self) -> &'static str {
        match self {
            Self::Today => "CALENDAR_WINDOW_TODAY",
            Self::Yesterday => "CALENDAR_WINDOW_YESTERDAY",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeRange {
    Fixed { from_ms: i64, to_ms: i64 },
    RelativeMinutes(i64),
    Calendar(CalendarWindow),
}

impl TimeRange {
    pub fn to_wire(self) -> Value {
        match self {
            Self::Fixed { from_ms, to_ms } => serde_json::json!({
                "fixed": {
                    "fromTimestampMs": from_ms.to_string(),
                    "toTimestampMs": to_ms.to_string(),
                }
            }),
            Self::RelativeMinutes(minutes) => {
                serde_json::json!({ "relativeMinutes": minutes.to_string() })
            }
            Self::Calendar(window) => serde_json::json!({ "calendar": window.wire() }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SavedObjectKind {
    Funnel,
    Pathway,
    Heatmap,
}

impl SavedObjectKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Funnel => "funnel",
            Self::Pathway => "pathway",
            Self::Heatmap => "heatmap",
        }
    }

    fn path(self) -> &'static str {
        match self {
            Self::Funnel => FUNNELS_PATH,
            Self::Pathway => PATHWAYS_PATH,
            Self::Heatmap => HEATMAPS_PATH,
        }
    }

    fn list_key(self) -> &'static str {
        match self {
            Self::Funnel => "funnels",
            Self::Pathway => "pathways",
            Self::Heatmap => "heatmaps",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryKind {
    Views,
    Events,
    Event,
    FunnelConversion,
    FunnelMatches,
    PathwayPaths,
    HeatmapTopElements,
}

impl QueryKind {
    fn path(self) -> String {
        match self {
            Self::Views => VIEWS_PATH.to_string(),
            Self::Events | Self::Event => EVENTS_PATH.to_string(),
            Self::FunnelConversion => format!("{FUNNELS_PATH}:queryConversion"),
            Self::FunnelMatches => format!("{FUNNELS_PATH}:listMatches"),
            Self::PathwayPaths => format!("{PATHWAYS_PATH}:queryPaths"),
            Self::HeatmapTopElements => format!("{HEATMAPS_PATH}:queryTopElements"),
        }
    }

    fn saved_id_key(self) -> Option<&'static str> {
        match self {
            Self::FunnelConversion | Self::FunnelMatches => Some("funnelId"),
            Self::PathwayPaths => Some("pathwayId"),
            Self::HeatmapTopElements => Some("heatmapId"),
            Self::Views | Self::Events | Self::Event => None,
        }
    }

    fn inline_key(self) -> Option<&'static str> {
        match self {
            Self::FunnelConversion | Self::FunnelMatches => Some("definition"),
            Self::PathwayPaths | Self::HeatmapTopElements => Some("view"),
            Self::Views | Self::Events | Self::Event => None,
        }
    }

    fn items_key(self, response: &Value) -> &'static str {
        match self {
            Self::Views => "views",
            Self::Events => "events",
            Self::FunnelMatches => match response.get("mode").and_then(Value::as_str) {
                Some("FUNNEL_MODE_SESSIONS") => "sessions",
                Some("FUNNEL_MODE_ACCOUNTS") => "accounts",
                _ => "users",
            },
            _ => "items",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct QueryInput {
    pub saved_id: Option<String>,
    pub app_name: Option<String>,
    pub view: Option<String>,
    pub time_range: Option<TimeRange>,
    pub source: Option<EventSource>,
    pub action_name_filter: Option<ActionNameFilter>,
    pub funnel_mode: Option<FunnelMode>,
    pub step_id: Option<String>,
    pub outcome: Option<FunnelOutcome>,
    pub user_id_mode: Option<UserIdMode>,
    pub direction: Option<PathwayDirection>,
    pub pages_count: Option<u32>,
    pub top_n: Option<u32>,
    pub page_size: Option<u32>,
    pub page_token: Option<String>,
}

pub fn build_query_body(
    kind: QueryKind,
    base: Option<Value>,
    input: &QueryInput,
) -> anyhow::Result<Value> {
    let mut body = match base {
        None => Map::new(),
        Some(Value::Object(map)) => map,
        Some(_) => bail!("the request file must hold a JSON object"),
    };
    let fields = [
        (kind.saved_id_key(), input.saved_id.clone().map(Value::from)),
        (Some("appName"), input.app_name.clone().map(Value::from)),
        (Some("view"), input.view.clone().map(Value::from)),
        (
            Some(TIME_RANGE_FIELD),
            input.time_range.map(TimeRange::to_wire),
        ),
        (Some("source"), input.source.map(|v| v.wire().into())),
        (
            Some("actionNameFilter"),
            input.action_name_filter.map(|v| v.wire().into()),
        ),
        (Some("mode"), input.funnel_mode.map(|v| v.wire().into())),
        (Some("stepId"), input.step_id.clone().map(Value::from)),
        (Some("outcome"), input.outcome.map(|v| v.wire().into())),
        (
            Some("userIdMode"),
            input.user_id_mode.map(|v| v.wire().into()),
        ),
        (Some("direction"), input.direction.map(|v| v.wire().into())),
        (Some("pagesCount"), input.pages_count.map(Value::from)),
        (Some("topN"), input.top_n.map(Value::from)),
        (Some("pageSize"), input.page_size.map(Value::from)),
        (Some("pageToken"), input.page_token.clone().map(Value::from)),
    ];
    for (key, value) in fields {
        if let (Some(key), Some(value)) = (key, value) {
            body.insert(key.to_string(), value);
        }
    }

    let runs_saved_object = kind
        .saved_id_key()
        .is_some_and(|key| body.contains_key(key));
    if let (Some(saved_key), Some(inline_key)) = (kind.saved_id_key(), kind.inline_key()) {
        match (runs_saved_object, body.contains_key(inline_key)) {
            (true, true) => {
                bail!("pass either a saved ID ({saved_key}) or an inline {inline_key}, not both")
            }
            (false, false) => {
                bail!("pass a saved ID, or an inline {inline_key} (flag or request file)")
            }
            _ => {}
        }
    }
    if !runs_saved_object && !body.contains_key(TIME_RANGE_FIELD) {
        body.insert(
            TIME_RANGE_FIELD.to_string(),
            TimeRange::RelativeMinutes(DEFAULT_TIME_RANGE_MINUTES).to_wire(),
        );
    }
    Ok(Value::Object(body))
}

pub fn prepare_update(body: Value, update_mask: Option<&str>) -> anyhow::Result<(Value, String)> {
    let Value::Object(mut map) = body else {
        bail!("the update file must hold a JSON object");
    };
    for field in READ_ONLY_FIELDS {
        map.remove(*field);
    }
    let mask = match update_mask {
        Some(mask) => mask.to_string(),
        None => {
            let mut fields: Vec<&str> = map.keys().map(String::as_str).collect();
            fields.sort_unstable();
            fields.join(",")
        }
    };
    if mask.is_empty() {
        bail!("nothing to update: the file has no updatable fields and --update-mask is empty");
    }
    Ok((Value::Object(map), mask))
}

#[derive(Debug, Default)]
pub struct Page {
    pub items: Vec<Value>,
    pub next_page_token: Option<String>,
    pub total_size: Option<String>,
    pub costly: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageMeta {
    #[serde(default)]
    next_page_token: String,
    #[serde(default, deserialize_with = "string_or_number")]
    total_size: Option<String>,
    #[serde(default)]
    costly: bool,
}

impl Page {
    fn from_response(response: Value, items_key: &str) -> Result<Self> {
        let meta: PageMeta = serde_json::from_value(response.clone())?;
        let items = match response.get(items_key) {
            Some(Value::Array(items)) => items.clone(),
            _ => Vec::new(),
        };
        Ok(Self {
            items,
            next_page_token: Some(meta.next_page_token).filter(|token| !token.is_empty()),
            total_size: meta.total_size,
            costly: meta.costly,
        })
    }
}

pub fn is_costly(response: &Value) -> bool {
    response.get("costly").and_then(Value::as_bool) == Some(true)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Application {
    pub app_name: Option<String>,
    #[serde(default, deserialize_with = "string_or_number")]
    pub sessions: Option<String>,
    #[serde(default, deserialize_with = "string_or_number")]
    pub crash_free_sessions: Option<String>,
    #[serde(default, deserialize_with = "string_or_number")]
    pub users: Option<String>,
    #[serde(default, deserialize_with = "string_or_number")]
    pub crash_free_users: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewSessions {
    pub view: Option<String>,
    #[serde(default, deserialize_with = "string_or_number")]
    pub sessions: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventRef {
    pub source: Option<String>,
    pub name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub event_id: Option<String>,
    pub event: Option<EventRef>,
    #[serde(default, deserialize_with = "string_or_number")]
    pub count: Option<String>,
    #[serde(default, deserialize_with = "string_or_number")]
    pub dead_clicks: Option<String>,
    pub dead_clicks_pct: Option<f64>,
    #[serde(default, deserialize_with = "string_or_number")]
    pub rage_clicks: Option<String>,
    pub rage_clicks_pct: Option<f64>,
    #[serde(default, deserialize_with = "string_or_number")]
    pub session_count: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryRumEventResponse {
    pub event: Option<Event>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FunnelDefinitionSummary {
    pub mode: Option<String>,
    #[serde(default)]
    pub steps: Vec<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedObject {
    #[serde(default, deserialize_with = "string_or_number")]
    pub id: Option<String>,
    pub app_name: Option<String>,
    pub name: Option<String>,
    pub view: Option<String>,
    pub view_name: Option<String>,
    pub direction: Option<String>,
    pub mode: Option<String>,
    pub definition: Option<FunnelDefinitionSummary>,
    pub owner: Option<String>,
    pub updated_at: Option<String>,
    pub version: Option<i64>,
}

impl SavedObject {
    pub fn details(&self) -> String {
        if let Some(definition) = &self.definition {
            return format!(
                "{}, {} steps",
                display_enum(definition.mode.as_deref()),
                definition.steps.len()
            );
        }
        let view = self
            .view
            .as_deref()
            .or(self.view_name.as_deref())
            .unwrap_or("-");
        let mode = self.direction.as_deref().or(self.mode.as_deref());
        format!("{view} {}", display_enum(mode))
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FunnelStepResult {
    pub step_id: Option<String>,
    pub label: Option<String>,
    #[serde(default, deserialize_with = "string_or_number")]
    pub count: Option<String>,
    pub converted_pct_of_total: Option<f64>,
    pub converted_pct_of_previous: Option<f64>,
    #[serde(default, deserialize_with = "string_or_number")]
    pub dropped_count: Option<String>,
    pub avg_time_from_previous: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FunnelDropOff {
    pub from_step_id: Option<String>,
    pub to_step_id: Option<String>,
    pub dropped_pct_of_previous: Option<f64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FunnelSummary {
    #[serde(default, deserialize_with = "string_or_number")]
    pub entered_count: Option<String>,
    #[serde(default, deserialize_with = "string_or_number")]
    pub converted_count: Option<String>,
    pub converted_pct: Option<f64>,
    pub biggest_drop_off: Option<FunnelDropOff>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FunnelGroupResult {
    pub group: Option<String>,
    #[serde(default)]
    pub steps: Vec<FunnelStepResult>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FunnelConversion {
    pub app_name: Option<String>,
    pub mode: Option<String>,
    pub summary: Option<FunnelSummary>,
    #[serde(default)]
    pub steps: Vec<FunnelStepResult>,
    #[serde(default)]
    pub groups: Vec<FunnelGroupResult>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryFunnelConversionResponse {
    pub conversion: Option<FunnelConversion>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FunnelMatchedUser {
    pub user_id: Option<String>,
    pub example_session_id: Option<String>,
    pub has_session_replay: Option<bool>,
    pub platform: Option<String>,
    pub os: Option<String>,
    pub country: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PathwayPath {
    #[serde(default)]
    pub pages: Vec<String>,
    #[serde(default, deserialize_with = "string_or_number")]
    pub count: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PathwayPaths {
    pub app_name: Option<String>,
    pub view: Option<String>,
    pub direction: Option<String>,
    #[serde(default)]
    pub paths: Vec<PathwayPath>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryPathwayPathsResponse {
    pub pathway_paths: Option<PathwayPaths>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HeatmapElement {
    pub event_id: Option<String>,
    pub name: Option<String>,
    #[serde(default, deserialize_with = "string_or_number")]
    pub clicks: Option<String>,
    pub dead_clicks_pct: Option<f64>,
    pub rage_clicks_pct: Option<f64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HeatmapTopElements {
    pub app_name: Option<String>,
    pub view: Option<String>,
    pub action_name_filter: Option<String>,
    #[serde(default)]
    pub elements: Vec<HeatmapElement>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryHeatmapTopElementsResponse {
    pub heatmap_data: Option<HeatmapTopElements>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    #[serde(default)]
    pub enabled: bool,
}

pub struct ProductAnalyticsApi<'a> {
    client: &'a CxClient,
}

impl<'a> ProductAnalyticsApi<'a> {
    pub fn new(client: &'a CxClient) -> Self {
        Self { client }
    }

    pub async fn list_applications(&self, app_names: &[String]) -> Result<Page> {
        let params: Vec<(&str, &str)> = app_names
            .iter()
            .map(|name| (APP_NAMES_PARAM, name.as_str()))
            .collect();
        let path = format!("{BASE_PATH}{APPLICATIONS_PATH}");
        let response: Value = self
            .client
            .get(&path, &params)
            .await
            .map_err(explain_disabled)?;
        Page::from_response(response, "applications")
    }

    pub async fn get_application(&self, app_name: &str) -> Result<Value> {
        let path = format!(
            "{BASE_PATH}{APPLICATIONS_PATH}/{}",
            encode_segment(app_name)
        );
        self.client.get(&path, &[]).await.map_err(explain_disabled)
    }

    pub async fn list_saved(
        &self,
        kind: SavedObjectKind,
        app_names: &[String],
        page_size: Option<u32>,
        page_token: Option<&str>,
    ) -> Result<Page> {
        let page_size = page_size.map(|size| size.to_string());
        let mut params: Vec<(&str, &str)> = app_names
            .iter()
            .map(|name| (APP_NAMES_PARAM, name.as_str()))
            .collect();
        if let Some(size) = page_size.as_deref() {
            params.push((PAGE_SIZE_PARAM, size));
        }
        if let Some(token) = page_token {
            params.push((PAGE_TOKEN_PARAM, token));
        }
        let path = format!("{BASE_PATH}{}", kind.path());
        let response: Value = self
            .client
            .get(&path, &params)
            .await
            .map_err(explain_disabled)?;
        Page::from_response(response, kind.list_key())
    }

    pub async fn get_saved(&self, kind: SavedObjectKind, id: &str) -> Result<Value> {
        let path = format!("{BASE_PATH}{}/{}", kind.path(), encode_segment(id));
        self.client.get(&path, &[]).await.map_err(explain_disabled)
    }

    pub async fn create_saved(&self, kind: SavedObjectKind, body: &Value) -> Result<Value> {
        let path = format!("{BASE_PATH}{}", kind.path());
        self.client
            .post(&path, body)
            .await
            .map_err(explain_disabled)
    }

    pub async fn update_saved(
        &self,
        kind: SavedObjectKind,
        id: &str,
        body: &Value,
        update_mask: &str,
    ) -> Result<Value> {
        let path = format!(
            "{BASE_PATH}{}/{}?{UPDATE_MASK_PARAM}={}",
            kind.path(),
            encode_segment(id),
            utf8_percent_encode(update_mask, QUERY_VALUE_ENCODE_SET)
        );
        self.client
            .patch(&path, body)
            .await
            .map_err(explain_disabled)
    }

    pub async fn delete_saved(&self, kind: SavedObjectKind, id: &str) -> Result<Value> {
        let path = format!("{BASE_PATH}{}/{}", kind.path(), encode_segment(id));
        self.client.delete(&path).await.map_err(explain_disabled)
    }

    pub async fn query(&self, kind: QueryKind, body: &Value) -> Result<Value> {
        let path = format!("{BASE_PATH}{}", kind.path());
        self.client
            .post(&path, body)
            .await
            .map_err(explain_disabled)
    }

    pub async fn query_page(&self, kind: QueryKind, body: &Value) -> Result<Page> {
        let response = self.query(kind, body).await?;
        let items_key = kind.items_key(&response);
        Page::from_response(response, items_key)
    }

    pub async fn query_event(&self, event_id: &str, body: &Value) -> Result<Value> {
        let path = format!(
            "{BASE_PATH}{}/{}",
            QueryKind::Event.path(),
            encode_segment(event_id)
        );
        self.client
            .post(&path, body)
            .await
            .map_err(explain_disabled)
    }

    pub async fn get_settings(&self) -> Result<Value> {
        let path = format!("{BASE_PATH}{SETTINGS_PATH}");
        self.client.get(&path, &[]).await
    }

    pub async fn update_settings(&self, enabled: bool) -> Result<Value> {
        let path =
            format!("{BASE_PATH}{SETTINGS_PATH}?{UPDATE_MASK_PARAM}={SETTINGS_ENABLED_FIELD}");
        let mut body = Map::new();
        body.insert(SETTINGS_ENABLED_FIELD.to_string(), Value::Bool(enabled));
        self.client.patch(&path, &Value::Object(body)).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn deserialize_application() {
        let app: Application = serde_json::from_value(json!({
            "appName": "web-shop",
            "sessions": "18420",
            "crashFreeSessions": "18391",
            "users": "6210",
            "crashFreeUsers": "6198"
        }))
        .unwrap();
        assert_eq!(app.app_name.as_deref(), Some("web-shop"));
        assert_eq!(app.sessions.as_deref(), Some("18420"));
        assert_eq!(app.crash_free_users.as_deref(), Some("6198"));
    }

    #[test]
    fn page_reads_items_token_and_costly() {
        let page = Page::from_response(
            json!({
                "views": [{"view": "/", "sessions": "18420"}],
                "nextPageToken": "eyJvIjo1MH0",
                "costly": true
            }),
            "views",
        )
        .unwrap();
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.next_page_token.as_deref(), Some("eyJvIjo1MH0"));
        assert!(page.costly);
        let view: ViewSessions = serde_json::from_value(page.items[0].clone()).unwrap();
        assert_eq!(view.sessions.as_deref(), Some("18420"));
    }

    #[test]
    fn page_treats_empty_token_and_missing_items_as_last_empty_page() {
        let page = Page::from_response(json!({"nextPageToken": ""}), "funnels").unwrap();
        assert!(page.items.is_empty());
        assert!(page.next_page_token.is_none());
        assert!(!page.costly);
    }

    #[test]
    fn page_reads_total_size_as_number_or_string() {
        let page = Page::from_response(json!({"funnels": [], "totalSize": 3}), "funnels").unwrap();
        assert_eq!(page.total_size.as_deref(), Some("3"));
    }

    #[test]
    fn matches_items_follow_response_mode() {
        let users = json!({"mode": "FUNNEL_MODE_USERS"});
        let sessions = json!({"mode": "FUNNEL_MODE_SESSIONS"});
        assert_eq!(QueryKind::FunnelMatches.items_key(&users), "users");
        assert_eq!(QueryKind::FunnelMatches.items_key(&sessions), "sessions");
    }

    #[test]
    fn deserialize_click_and_custom_events() {
        let click: Event = serde_json::from_value(json!({
            "eventId": "ev_Y2xpY2s6R290IGl0",
            "event": {"source": "EVENT_SOURCE_AUTO", "name": "Got it"},
            "count": "5",
            "deadClicks": "0",
            "deadClicksPct": 0,
            "rageClicks": "2",
            "rageClicksPct": 40
        }))
        .unwrap();
        assert_eq!(click.count.as_deref(), Some("5"));
        assert_eq!(click.rage_clicks_pct, Some(40.0));
        let custom: Event = serde_json::from_value(json!({
            "eventId": "ev_abc",
            "event": {"source": "EVENT_SOURCE_CUSTOM", "name": "signup"},
            "sessionCount": "12"
        }))
        .unwrap();
        assert_eq!(custom.session_count.as_deref(), Some("12"));
        assert!(custom.count.is_none());
    }

    #[test]
    fn deserialize_saved_funnel_details() {
        let funnel: SavedObject = serde_json::from_value(json!({
            "id": "42",
            "appName": "web-shop",
            "name": "Checkout funnel",
            "definition": {
                "mode": "FUNNEL_MODE_USERS",
                "conversionWindow": "3600s",
                "steps": [{"stepId": "s1"}, {"stepId": "s2"}]
            },
            "filters": [],
            "timeRange": {"relativeMinutes": "1440"},
            "createdAt": "2026-11-16T10:00:00Z",
            "updatedAt": "2026-11-16T10:00:00Z",
            "version": 1
        }))
        .unwrap();
        assert_eq!(funnel.id.as_deref(), Some("42"));
        assert_eq!(funnel.details(), "USERS, 2 steps");
        assert_eq!(funnel.version, Some(1));
    }

    #[test]
    fn saved_pathway_and_heatmap_details() {
        let pathway: SavedObject = serde_json::from_value(json!({
            "id": "44", "view": "/cart", "direction": "PATHWAY_DIRECTION_AFTER"
        }))
        .unwrap();
        assert_eq!(pathway.details(), "/cart AFTER");
        let heatmap: SavedObject = serde_json::from_value(json!({
            "id": "43", "viewName": "/checkout", "mode": "HEATMAP_MODE_CLICK_MAP"
        }))
        .unwrap();
        assert_eq!(heatmap.details(), "/checkout CLICK_MAP");
    }

    #[test]
    fn deserialize_funnel_conversion() {
        let response: QueryFunnelConversionResponse = serde_json::from_value(json!({
            "conversion": {
                "appName": "web-shop",
                "mode": "FUNNEL_MODE_USERS",
                "summary": {
                    "enteredCount": "30122",
                    "convertedCount": "21877",
                    "convertedPct": 72.6,
                    "droppedCount": "8245",
                    "droppedPct": 27.4,
                    "biggestDropOff": {
                        "fromStepId": "s1",
                        "toStepId": "s2",
                        "droppedCount": "8245",
                        "droppedPctOfPrevious": 27.4
                    }
                },
                "steps": [
                    {"stepId": "s1", "label": "Viewed cart", "count": "30122",
                     "convertedPctOfTotal": 100, "convertedPctOfPrevious": 100,
                     "droppedCount": "0", "droppedPctOfTotal": 0, "droppedPctOfPrevious": 0},
                    {"stepId": "s2", "count": "21877", "convertedPctOfTotal": 72.6,
                     "convertedPctOfPrevious": 72.6, "droppedCount": "8245",
                     "droppedPctOfTotal": 27.4, "droppedPctOfPrevious": 27.4,
                     "avgTimeFromPrevious": "192s"}
                ],
                "groups": []
            },
            "costly": false
        }))
        .unwrap();
        let conversion = response.conversion.unwrap();
        assert_eq!(conversion.steps.len(), 2);
        assert_eq!(
            conversion.summary.unwrap().converted_count.as_deref(),
            Some("21877")
        );
        assert_eq!(
            conversion.steps[1].avg_time_from_previous.as_deref(),
            Some("192s")
        );
    }

    #[test]
    fn deserialize_pathway_paths_and_heatmap_elements() {
        let paths: QueryPathwayPathsResponse = serde_json::from_value(json!({
            "pathwayPaths": {
                "appName": "docs", "view": "/setup", "direction": "PATHWAY_DIRECTION_AFTER",
                "paths": [{"pages": ["/setup", "/tools"], "count": "41"}]
            },
            "costly": false
        }))
        .unwrap();
        assert_eq!(paths.pathway_paths.unwrap().paths[0].pages.len(), 2);
        let heatmap: QueryHeatmapTopElementsResponse = serde_json::from_value(json!({
            "heatmapData": {
                "appName": "web-app", "view": "explore",
                "actionNameFilter": "ACTION_NAME_FILTER_ALL_CLICKS",
                "elements": [{"eventId": "ev_Y2xpY2s6cnVu", "name": "run", "clicks": "12420",
                              "deadClicksPct": 0, "rageClicksPct": 1}]
            },
            "costly": false
        }))
        .unwrap();
        let elements = heatmap.heatmap_data.unwrap().elements;
        assert_eq!(elements[0].clicks.as_deref(), Some("12420"));
    }

    #[test]
    fn settings_missing_enabled_reads_false() {
        let settings: Settings = serde_json::from_value(json!({})).unwrap();
        assert!(!settings.enabled);
    }

    #[test]
    fn time_range_wire_shapes() {
        assert_eq!(
            TimeRange::RelativeMinutes(1440).to_wire(),
            json!({"relativeMinutes": "1440"})
        );
        assert_eq!(
            TimeRange::Fixed {
                from_ms: 1,
                to_ms: 2
            }
            .to_wire(),
            json!({"fixed": {"fromTimestampMs": "1", "toTimestampMs": "2"}})
        );
        assert_eq!(
            TimeRange::Calendar(CalendarWindow::Today).to_wire(),
            json!({"calendar": "CALENDAR_WINDOW_TODAY"})
        );
    }

    #[test]
    fn query_body_merges_flags_over_file_and_defaults_time_range() {
        let input = QueryInput {
            app_name: Some("web-shop".into()),
            view: Some("/cart".into()),
            source: Some(EventSource::Custom),
            page_size: Some(10),
            ..Default::default()
        };
        let body = build_query_body(
            QueryKind::Events,
            Some(json!({"appName": "other", "filters": [{"boolFilter": {"field": "BOOL_FILTER_FIELD_HAS_SCREENSHOT", "value": true}}]})),
            &input,
        )
        .unwrap();
        assert_eq!(body["appName"], "web-shop");
        assert_eq!(body["source"], "EVENT_SOURCE_CUSTOM");
        assert_eq!(body["pageSize"], 10);
        assert_eq!(body["filters"].as_array().unwrap().len(), 1);
        assert_eq!(body["timeRange"], json!({"relativeMinutes": "1440"}));
    }

    #[test]
    fn saved_query_keeps_saved_time_range() {
        let input = QueryInput {
            saved_id: Some("42".into()),
            ..Default::default()
        };
        let body = build_query_body(QueryKind::FunnelConversion, None, &input).unwrap();
        assert_eq!(body, json!({"funnelId": "42"}));
    }

    #[test]
    fn query_requires_exactly_one_of_saved_id_or_inline() {
        assert!(build_query_body(QueryKind::PathwayPaths, None, &QueryInput::default()).is_err());
        let both = QueryInput {
            saved_id: Some("44".into()),
            view: Some("/cart".into()),
            ..Default::default()
        };
        assert!(build_query_body(QueryKind::PathwayPaths, None, &both).is_err());
        let inline = QueryInput {
            view: Some("/cart".into()),
            ..Default::default()
        };
        let body = build_query_body(QueryKind::HeatmapTopElements, None, &inline).unwrap();
        assert_eq!(body["view"], "/cart");
    }

    #[test]
    fn query_file_must_be_an_object() {
        assert!(
            build_query_body(QueryKind::Views, Some(json!([])), &QueryInput::default()).is_err()
        );
    }

    #[test]
    fn update_strips_read_only_fields_and_derives_mask() {
        let (body, mask) = prepare_update(
            json!({"id": "42", "appName": "web-shop", "name": "Renamed", "version": 3, "filters": []}),
            None,
        )
        .unwrap();
        assert_eq!(body, json!({"name": "Renamed", "filters": []}));
        assert_eq!(mask, "filters,name");
    }

    #[test]
    fn update_keeps_explicit_mask() {
        let (_, mask) =
            prepare_update(json!({"name": "x", "breakdown": null}), Some("breakdown")).unwrap();
        assert_eq!(mask, "breakdown");
    }

    #[test]
    fn update_with_nothing_to_update_fails() {
        assert!(prepare_update(json!({"id": "42"}), None).is_err());
    }

    #[test]
    fn disabled_error_gets_a_hint() {
        let error = explain_disabled(CxError::Api {
            status: 412,
            message: "Product Analytics is not enabled for this team.".into(),
        });
        assert!(error
            .to_string()
            .contains("cx product-analytics settings get"));
        let other = explain_disabled(CxError::Api {
            status: 404,
            message: "not found".into(),
        });
        assert!(!other.to_string().contains("settings"));
    }

    #[test]
    fn display_enum_strips_known_prefixes() {
        assert_eq!(display_enum(Some("FUNNEL_MODE_USERS")), "USERS");
        assert_eq!(display_enum(Some("UNKNOWN_VALUE")), "UNKNOWN_VALUE");
        assert_eq!(display_enum(None), "-");
    }
}
