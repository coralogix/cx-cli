use anyhow::bail;
use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::api_client::CxClient;
use crate::error::Result;
use crate::serde_helpers::string_or_number;

const BASE_PATH: &str = "/product-analytics";
const SETTINGS_PATH: &str = "/settings/v1";
const SAVED_PATH: &str = "/saved";
const SAVED_SEARCH_PATH: &str = "/saved/search";

const APP_NAME_FIELD: &str = "appName";
const APP_NAMES_FIELD: &str = "appNames";
const FROM_TIMESTAMP_FIELD: &str = "fromTimestamp";
const TO_TIMESTAMP_FIELD: &str = "toTimestamp";
const QUERY_FIELD: &str = "query";
const STEPS_FIELD: &str = "steps";
const TIME_RANGE_FIELD: &str = "timeRange";
const SETTINGS_ENABLED_FIELD: &str = "enabled";
const READ_ONLY_FIELDS: &[&str] = &["id", APP_NAME_FIELD, "createdAt"];

const ENUM_PREFIXES: &[&str] = &["GROUP_BY_", "PATHWAYS_MODE_", "CALENDAR_WINDOW_"];
const SECONDS_PER_DAY: i64 = 86_400;

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
pub enum ActionType {
    Click,
    Custom,
}

impl ActionType {
    fn field(self) -> &'static str {
        match self {
            Self::Click => "click",
            Self::Custom => "custom",
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
            Self::All => "ALL_CLICKS",
            Self::Named => "NAMED_ONLY",
            Self::Unnamed => "UNNAMED_ONLY",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum FunnelGroupBy {
    Sessions,
    Users,
    Accounts,
}

impl FunnelGroupBy {
    pub fn wire(self) -> &'static str {
        match self {
            Self::Sessions => "GROUP_BY_SESSION_ID",
            Self::Users => "GROUP_BY_CX_USER_ID",
            Self::Accounts => "GROUP_BY_ACCOUNT_ID",
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
            Self::Converted => "CONVERTED",
            Self::Dropped => "DROPOFF",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum PathwayDirection {
    Forward,
    Backward,
}

impl PathwayDirection {
    pub fn wire(self) -> &'static str {
        match self {
            Self::Forward => "PATHWAYS_MODE_FORWARD",
            Self::Backward => "PATHWAYS_MODE_BACKWARD",
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
            Self::Funnel => "/funnels/v1",
            Self::Pathway => "/pathways/v1",
            Self::Heatmap => "/heatmaps/v1",
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
    Applications,
    Views,
    Actions,
    FilterValues,
    Funnel,
    MatchingUsers,
    Pathways,
    Clicks,
    ScrollMap,
}

impl QueryKind {
    fn path(self) -> &'static str {
        match self {
            Self::Applications => "/applications/v1",
            Self::Views => "/views/v1",
            Self::Actions => "/actions/v1",
            Self::FilterValues => "/filter-values/v1",
            Self::Funnel => "/funnels/v1/query",
            Self::MatchingUsers => "/funnels/v1/matching-users",
            Self::Pathways => "/pathways/v1/query",
            Self::Clicks => "/clicks/v1",
            Self::ScrollMap => "/scroll-map/v1",
        }
    }

    pub fn items_key(self) -> &'static str {
        match self {
            Self::Applications => "apps",
            Self::Views => "views",
            Self::Actions => "actions",
            Self::FilterValues => "values",
            Self::Funnel => "groups",
            Self::MatchingUsers => "users",
            Self::Pathways => "pathways",
            Self::Clicks => "clicks",
            Self::ScrollMap => "pageCovers",
        }
    }

    pub fn saved_kind(self) -> Option<SavedObjectKind> {
        match self {
            Self::Funnel | Self::MatchingUsers => Some(SavedObjectKind::Funnel),
            Self::Pathways => Some(SavedObjectKind::Pathway),
            Self::Clicks | Self::ScrollMap => Some(SavedObjectKind::Heatmap),
            Self::Applications | Self::Views | Self::Actions | Self::FilterValues => None,
        }
    }

    fn saved_fields(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::Funnel | Self::MatchingUsers => &[
                (STEPS_FIELD, STEPS_FIELD),
                ("filters", "filters"),
                ("conversionWindowSeconds", "conversionWindowSeconds"),
            ],
            Self::Pathways => &[
                ("pageName", "pageName"),
                ("pagesCount", "pagesCount"),
                ("limit", "limit"),
                ("filters", "filters"),
                ("mode", "mode"),
            ],
            Self::Clicks => &[
                ("viewName", "pageName"),
                ("filters", "filters"),
                ("actionNameFilter", "actionNameFilter"),
            ],
            Self::ScrollMap => &[("viewName", "pageName"), ("filters", "filters")],
            Self::Applications | Self::Views | Self::Actions | Self::FilterValues => &[],
        }
    }

    fn set_app(self, body: &mut Map<String, Value>, app: &str) {
        if matches!(self, Self::Clicks | Self::ScrollMap) {
            body.insert(APP_NAME_FIELD.to_string(), json!(app));
        } else {
            body.insert(APP_NAMES_FIELD.to_string(), json!([app]));
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct QueryInput {
    pub app_name: Option<String>,
    pub view: Option<String>,
    pub time_range: Option<(i64, i64)>,
    pub filter_field: Option<String>,
    pub action_type: Option<ActionType>,
    pub action_name_filter: Option<ActionNameFilter>,
    pub group_by: Option<FunnelGroupBy>,
    pub outcome: Option<FunnelOutcome>,
    pub start_step: Option<u32>,
    pub direction: Option<PathwayDirection>,
    pub pages_count: Option<u32>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
    pub top_n: Option<u32>,
}

fn insert_some(body: &mut Map<String, Value>, fields: Vec<(&str, Option<Value>)>) {
    for (key, value) in fields {
        if let Some(value) = value {
            body.insert(key.to_string(), value);
        }
    }
}

pub fn build_query_body(
    kind: QueryKind,
    base: Option<Value>,
    input: &QueryInput,
    time_range: (i64, i64),
) -> anyhow::Result<Value> {
    let mut body = match base {
        None => Map::new(),
        Some(Value::Object(map)) => map,
        Some(_) => bail!("the request file must hold a JSON object"),
    };
    if matches!(kind, QueryKind::Funnel | QueryKind::MatchingUsers) {
        return build_funnel_body(kind, body, input, time_range);
    }
    if let Some(app) = &input.app_name {
        kind.set_app(&mut body, app);
    }
    body.insert(FROM_TIMESTAMP_FIELD.to_string(), json!(time_range.0));
    body.insert(TO_TIMESTAMP_FIELD.to_string(), json!(time_range.1));
    let action_name_filter = input.action_name_filter.map(|v| json!(v.wire()));
    if kind == QueryKind::Actions {
        let action_type = input.action_type.unwrap_or(ActionType::Click);
        let mut action = Map::new();
        insert_some(
            &mut action,
            vec![("actionNameFilter", action_name_filter.clone())],
        );
        body.insert(action_type.field().to_string(), Value::Object(action));
    } else {
        insert_some(&mut body, vec![("actionNameFilter", action_name_filter)]);
    }
    insert_some(
        &mut body,
        vec![
            ("pageName", input.view.clone().map(Value::from)),
            ("filterField", input.filter_field.clone().map(Value::from)),
            ("mode", input.direction.map(|v| json!(v.wire()))),
            ("pagesCount", input.pages_count.map(Value::from)),
            ("limit", input.limit.map(Value::from)),
            ("offset", input.offset.map(Value::from)),
            ("topK", input.top_n.map(Value::from)),
        ],
    );
    Ok(Value::Object(body))
}

fn build_funnel_body(
    kind: QueryKind,
    mut body: Map<String, Value>,
    input: &QueryInput,
    time_range: (i64, i64),
) -> anyhow::Result<Value> {
    let Some(Value::Object(mut query)) = body.remove(QUERY_FIELD) else {
        bail!("the funnel request needs a \"query\" object with the steps");
    };
    if let Some(app) = &input.app_name {
        kind.set_app(&mut query, app);
    }
    query.insert(FROM_TIMESTAMP_FIELD.to_string(), json!(time_range.0));
    query.insert(TO_TIMESTAMP_FIELD.to_string(), json!(time_range.1));
    if kind == QueryKind::Funnel {
        insert_some(
            &mut body,
            vec![("groupBy", input.group_by.map(|v| json!(v.wire())))],
        );
        body.insert(QUERY_FIELD.to_string(), Value::Object(query));
        return Ok(Value::Object(body));
    }
    let step_count = query
        .get(STEPS_FIELD)
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    if step_count == 0 {
        bail!("the funnel has no steps");
    }
    let mut request = Map::new();
    request.insert(QUERY_FIELD.to_string(), Value::Object(query));
    request.insert(
        "startStep".to_string(),
        json!(input.start_step.unwrap_or(1)),
    );
    request.insert("endStep".to_string(), json!(step_count));
    insert_some(
        &mut request,
        vec![
            ("mode", input.outcome.map(|v| json!(v.wire()))),
            ("limit", input.limit.map(Value::from)),
            ("offset", input.offset.map(Value::from)),
        ],
    );
    Ok(Value::Object(request))
}

pub fn request_from_saved(kind: QueryKind, saved: &Value) -> Value {
    let mut body = Map::new();
    for (from, to) in kind.saved_fields() {
        if let Some(value) = saved.get(*from) {
            body.insert(to.to_string(), value.clone());
        }
    }
    if let Some(app) = saved.get(APP_NAME_FIELD).and_then(Value::as_str) {
        kind.set_app(&mut body, app);
    }
    if !matches!(kind, QueryKind::Funnel | QueryKind::MatchingUsers) {
        return Value::Object(body);
    }
    let mut request = Map::new();
    request.insert(QUERY_FIELD.to_string(), Value::Object(body));
    for field in ["groupBy", "breakdown"] {
        if let Some(value) = saved.get(field) {
            request.insert(field.to_string(), value.clone());
        }
    }
    Value::Object(request)
}

fn int64(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
}

pub fn resolve_saved_time_range(saved: &Value, now: i64) -> anyhow::Result<(i64, i64)> {
    let time_range = &saved[TIME_RANGE_FIELD];
    if let Some(fixed) = time_range.get("fixed") {
        let (Some(from), Some(to)) = (
            int64(&fixed[FROM_TIMESTAMP_FIELD]),
            int64(&fixed[TO_TIMESTAMP_FIELD]),
        ) else {
            bail!(
                "the saved fixed time range has no start or end: {fixed}; pass --start and --end"
            );
        };
        return Ok((from, to));
    }
    if let Some(minutes) = int64(&time_range["relativeMinutes"]) {
        return Ok((now - minutes * 60, now));
    }
    let midnight = now - now.rem_euclid(SECONDS_PER_DAY);
    match time_range.get("calendar").and_then(Value::as_str) {
        Some("CALENDAR_WINDOW_TODAY") => Ok((midnight, now)),
        Some("CALENDAR_WINDOW_YESTERDAY") => Ok((midnight - SECONDS_PER_DAY, midnight)),
        _ => bail!("unsupported saved time range {time_range}; pass --start and --end"),
    }
}

pub fn strip_read_only_fields(body: Value) -> anyhow::Result<Value> {
    let Value::Object(mut map) = body else {
        bail!("the update file must hold a JSON object");
    };
    for field in READ_ONLY_FIELDS {
        map.remove(*field);
    }
    Ok(Value::Object(map))
}

pub fn items(response: &Value, key: &str) -> Vec<Value> {
    match response.get(key) {
        Some(Value::Array(items)) => items.clone(),
        _ => Vec::new(),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Application {
    pub app_name: Option<String>,
    #[serde(default, deserialize_with = "string_or_number")]
    pub session_count: Option<String>,
    #[serde(default, deserialize_with = "string_or_number")]
    pub crash_free_session_count: Option<String>,
    #[serde(default, deserialize_with = "string_or_number")]
    pub total_users: Option<String>,
    #[serde(default, deserialize_with = "string_or_number")]
    pub crash_free_user_count: Option<String>,
    pub platform: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewResult {
    pub view_name: Option<String>,
    #[serde(default, deserialize_with = "string_or_number")]
    pub session_count: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionResult {
    pub event_type: Option<String>,
    pub event_target: Option<String>,
    #[serde(default, deserialize_with = "string_or_number")]
    pub session_count: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilterValue {
    pub value: Option<String>,
    #[serde(default, deserialize_with = "string_or_number")]
    pub count: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FunnelStepResult {
    #[serde(default)]
    pub step_number: i64,
    #[serde(default, deserialize_with = "string_or_number")]
    pub session_count: Option<String>,
    #[serde(default)]
    pub conversion_rate: f64,
    #[serde(default, deserialize_with = "string_or_number")]
    pub avg_time_from_prev_ms: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FunnelGroupResult {
    pub group_value: Option<String>,
    #[serde(default)]
    pub steps: Vec<FunnelStepResult>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FunnelResponse {
    #[serde(default)]
    pub groups: Vec<FunnelGroupResult>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchingUser {
    pub user_id: Option<String>,
    pub user_name: Option<String>,
    pub user_email: Option<String>,
    pub example_session_id: Option<String>,
    pub has_session_replay: Option<bool>,
    pub os: Option<String>,
    pub platform: Option<String>,
    pub country_name: Option<String>,
    pub account_name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PathwayResult {
    #[serde(default)]
    pub pages: Vec<String>,
    #[serde(default, deserialize_with = "string_or_number")]
    pub count: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClickResult {
    pub click_name: Option<String>,
    pub cx_action_name: Option<String>,
    #[serde(default, deserialize_with = "string_or_number")]
    pub click_count: Option<String>,
    #[serde(default, deserialize_with = "string_or_number")]
    pub dead_click_count: Option<String>,
    #[serde(default, deserialize_with = "string_or_number")]
    pub rage_click_count: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageCover {
    #[serde(default)]
    pub page_percentage: i64,
    #[serde(default, deserialize_with = "string_or_number")]
    pub user_count: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScrollMapResponse {
    #[serde(default)]
    pub page_covers: Vec<PageCover>,
    #[serde(default)]
    pub avg_viewport_height: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedObject {
    #[serde(default, deserialize_with = "string_or_number")]
    pub id: Option<String>,
    pub app_name: Option<String>,
    pub name: Option<String>,
    #[serde(default)]
    pub steps: Vec<Value>,
    pub group_by: Option<String>,
    pub page_name: Option<String>,
    pub view_name: Option<String>,
    pub mode: Option<String>,
    #[serde(default, deserialize_with = "string_or_number")]
    pub created_at: Option<String>,
}

impl SavedObject {
    pub fn details(&self) -> String {
        if !self.steps.is_empty() {
            return format!(
                "{} steps, {}",
                self.steps.len(),
                display_enum(Some(
                    self.group_by.as_deref().unwrap_or("GROUP_BY_SESSION_ID")
                ))
            );
        }
        if let Some(page) = &self.page_name {
            let mode = self.mode.as_deref().unwrap_or("PATHWAYS_MODE_FORWARD");
            return format!("{page} {}", display_enum(Some(mode)));
        }
        format!(
            "{} {}",
            self.view_name.as_deref().unwrap_or("-"),
            display_enum(self.mode.as_deref())
        )
    }

    pub fn created(&self) -> String {
        self.created_at
            .as_deref()
            .and_then(|millis| millis.parse::<i64>().ok())
            .and_then(chrono::DateTime::from_timestamp_millis)
            .map(|time| time.to_rfc3339())
            .unwrap_or_else(|| "-".to_string())
    }
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

    pub async fn query(&self, kind: QueryKind, body: &Value) -> Result<Value> {
        let path = format!("{BASE_PATH}{}", kind.path());
        self.client.post(&path, body).await
    }

    async fn search_saved(&self, kind: SavedObjectKind, selector: &Value) -> Result<Vec<Value>> {
        let path = format!("{BASE_PATH}{}{SAVED_SEARCH_PATH}", kind.path());
        let response: Value = self.client.post(&path, selector).await?;
        Ok(items(&response, kind.list_key()))
    }

    pub async fn list_saved(
        &self,
        kind: SavedObjectKind,
        app_names: &[String],
    ) -> Result<Vec<Value>> {
        self.search_saved(kind, &json!({"appNames": {"values": app_names}}))
            .await
    }

    pub async fn get_saved(&self, kind: SavedObjectKind, id: i64) -> Result<Option<Value>> {
        let found = self
            .search_saved(kind, &json!({"ids": {"ids": [id]}}))
            .await?;
        Ok(found.into_iter().next())
    }

    pub async fn create_saved(&self, kind: SavedObjectKind, body: &Value) -> Result<Value> {
        let path = format!("{BASE_PATH}{}{SAVED_PATH}", kind.path());
        self.client.post(&path, body).await
    }

    pub async fn update_saved(
        &self,
        kind: SavedObjectKind,
        id: i64,
        body: &Value,
    ) -> Result<Value> {
        let path = format!("{BASE_PATH}{}{SAVED_PATH}/{id}", kind.path());
        self.client.put(&path, body).await
    }

    pub async fn delete_saved(&self, kind: SavedObjectKind, id: i64) -> Result<Value> {
        let path = format!("{BASE_PATH}{}{SAVED_PATH}/{id}", kind.path());
        self.client.delete(&path).await
    }

    pub async fn get_settings(&self) -> Result<Value> {
        let path = format!("{BASE_PATH}{SETTINGS_PATH}");
        self.client.get(&path, &[]).await
    }

    pub async fn update_settings(&self, enabled: bool) -> Result<Value> {
        let path = format!("{BASE_PATH}{SETTINGS_PATH}");
        let mut body = Map::new();
        body.insert(SETTINGS_ENABLED_FIELD.to_string(), Value::Bool(enabled));
        self.client.put(&path, &Value::Object(body)).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RANGE: (i64, i64) = (1_763_251_200, 1_763_337_600);

    #[test]
    fn deserialize_application_with_int64_strings_and_omitted_zeros() {
        let app: Application = serde_json::from_value(json!({
            "appName": "web-shop",
            "sessionCount": "18420",
            "totalUsers": "6210",
            "platform": "browser"
        }))
        .unwrap();
        assert_eq!(app.app_name.as_deref(), Some("web-shop"));
        assert_eq!(app.platform.as_deref(), Some("browser"));
        assert_eq!(app.session_count.as_deref(), Some("18420"));
        assert!(app.crash_free_session_count.is_none());
    }

    #[test]
    fn deserialize_views_actions_and_filter_values() {
        let view: ViewResult =
            serde_json::from_value(json!({"viewName": "explore", "sessionCount": "35814"}))
                .unwrap();
        assert_eq!(view.view_name.as_deref(), Some("explore"));
        let action: ActionResult = serde_json::from_value(
            json!({"eventType": "click", "eventTarget": "Log in with SSO", "sessionCount": 4459}),
        )
        .unwrap();
        assert_eq!(action.session_count.as_deref(), Some("4459"));
        let value: FilterValue =
            serde_json::from_value(json!({"value": "coralogix.com", "count": "102853"})).unwrap();
        assert_eq!(value.count.as_deref(), Some("102853"));
    }

    #[test]
    fn deserialize_funnel_groups() {
        let response: FunnelResponse = serde_json::from_value(json!({
            "groups": [{"groupValue": "Chrome", "steps": [
                {"stepNumber": 1, "sessionCount": "12259", "conversionRate": 1.0},
                {"stepNumber": 2, "sessionCount": "3706", "conversionRate": 0.3023085, "avgTimeFromPrevMs": "51730"}
            ]}]
        }))
        .unwrap();
        let steps = &response.groups[0].steps;
        assert_eq!(steps[1].step_number, 2);
        assert_eq!(steps[1].avg_time_from_prev_ms.as_deref(), Some("51730"));
        assert!(steps[0].avg_time_from_prev_ms.is_none());
        let empty: FunnelResponse = serde_json::from_value(json!({})).unwrap();
        assert!(empty.groups.is_empty());
    }

    #[test]
    fn deserialize_matching_user_clicks_pathways_and_scroll_map() {
        let user: MatchingUser = serde_json::from_value(json!({
            "userId": "u1", "userName": "Dana", "userEmail": "dana@example.com",
            "exampleSessionId": "s1", "hasSessionReplay": true, "countryName": "Germany",
            "exampleSessionCreationTime": "1763251200"
        }))
        .unwrap();
        assert_eq!(user.user_email.as_deref(), Some("dana@example.com"));
        let click: ClickResult = serde_json::from_value(json!({
            "clickName": "run", "clickCount": "17575", "rageClickCount": "3",
            "coordinates": [{"coordinate": {"height": 10}, "count": "4"}]
        }))
        .unwrap();
        assert_eq!(click.click_count.as_deref(), Some("17575"));
        assert!(click.dead_click_count.is_none());
        let pathway: PathwayResult =
            serde_json::from_value(json!({"pages": ["login/user", "explore"], "count": "778"}))
                .unwrap();
        assert_eq!(pathway.pages.len(), 2);
        let scroll: ScrollMapResponse = serde_json::from_value(json!({
            "pageCovers": [{"userCount": "8728"}, {"pagePercentage": 100, "userCount": "8723"}],
            "avgViewportHeight": 957
        }))
        .unwrap();
        assert_eq!(scroll.page_covers[0].page_percentage, 0);
        assert_eq!(scroll.avg_viewport_height, 957);
    }

    #[test]
    fn saved_object_details_and_created() {
        let funnel: SavedObject = serde_json::from_value(json!({
            "id": "21", "appName": "web-app", "name": "APM",
            "steps": [{"conditions": []}, {"conditions": []}],
            "createdAt": "1783433258498", "timeRange": {"relativeMinutes": "720"}
        }))
        .unwrap();
        assert_eq!(funnel.details(), "2 steps, SESSION_ID");
        assert!(funnel.created().starts_with("2026-07-"));
        let pathway: SavedObject =
            serde_json::from_value(json!({"id": "2", "pageName": "explore/metrics"})).unwrap();
        assert_eq!(pathway.details(), "explore/metrics FORWARD");
        let heatmap: SavedObject =
            serde_json::from_value(json!({"id": "3", "viewName": "alerts", "mode": "CLICK_MAP"}))
                .unwrap();
        assert_eq!(heatmap.details(), "alerts CLICK_MAP");
        assert_eq!(heatmap.created(), "-");
    }

    #[test]
    fn settings_missing_enabled_reads_false() {
        let settings: Settings = serde_json::from_value(json!({})).unwrap();
        assert!(!settings.enabled);
    }

    #[test]
    fn views_body_merges_flags_over_file_with_seconds() {
        let input = QueryInput {
            app_name: Some("web-shop".into()),
            offset: Some(1000),
            ..Default::default()
        };
        let body = build_query_body(
            QueryKind::Views,
            Some(json!({"appNames": ["other"], "filters": [{"field": "PLATFORM", "stringCondition": {"values": ["browser"]}}]})),
            &input,
            RANGE,
        )
        .unwrap();
        assert_eq!(
            body,
            json!({
                "appNames": ["web-shop"],
                "filters": [{"field": "PLATFORM", "stringCondition": {"values": ["browser"]}}],
                "fromTimestamp": RANGE.0,
                "toTimestamp": RANGE.1,
                "offset": 1000
            })
        );
    }

    #[test]
    fn actions_body_nests_the_click_filter() {
        let input = QueryInput {
            app_name: Some("web-app".into()),
            action_name_filter: Some(ActionNameFilter::Named),
            ..Default::default()
        };
        let body = build_query_body(QueryKind::Actions, None, &input, RANGE).unwrap();
        assert_eq!(body["click"], json!({"actionNameFilter": "NAMED_ONLY"}));
        let custom = QueryInput {
            app_name: Some("web-app".into()),
            action_type: Some(ActionType::Custom),
            ..Default::default()
        };
        let body = build_query_body(QueryKind::Actions, None, &custom, RANGE).unwrap();
        assert_eq!(body["custom"], json!({}));
        assert!(body.get("click").is_none());
    }

    #[test]
    fn clicks_body_uses_singular_app_name_and_top_k() {
        let input = QueryInput {
            app_name: Some("web-app".into()),
            view: Some("explore".into()),
            top_n: Some(3),
            ..Default::default()
        };
        let body = build_query_body(QueryKind::Clicks, None, &input, RANGE).unwrap();
        assert_eq!(
            body,
            json!({"appName": "web-app", "pageName": "explore", "topK": 3,
                   "fromTimestamp": RANGE.0, "toTimestamp": RANGE.1})
        );
    }

    #[test]
    fn funnel_body_puts_app_and_time_inside_the_query() {
        let input = QueryInput {
            app_name: Some("web-app".into()),
            group_by: Some(FunnelGroupBy::Users),
            ..Default::default()
        };
        let body = build_query_body(
            QueryKind::Funnel,
            Some(json!({"query": {"steps": [{"conditions": []}]}, "breakdown": {"field": "OS", "limit": 3}})),
            &input,
            RANGE,
        )
        .unwrap();
        assert_eq!(body["query"]["appNames"], json!(["web-app"]));
        assert_eq!(body["query"]["fromTimestamp"], RANGE.0);
        assert_eq!(body["groupBy"], "GROUP_BY_CX_USER_ID");
        assert_eq!(body["breakdown"]["field"], "OS");
    }

    #[test]
    fn matching_users_body_ends_at_the_last_step_and_drops_funnel_only_fields() {
        let input = QueryInput {
            outcome: Some(FunnelOutcome::Dropped),
            start_step: Some(2),
            limit: Some(10),
            ..Default::default()
        };
        let body = build_query_body(
            QueryKind::MatchingUsers,
            Some(json!({"query": {"appNames": ["web-app"], "steps": [{}, {}, {}]}, "groupBy": "GROUP_BY_CX_USER_ID"})),
            &input,
            RANGE,
        )
        .unwrap();
        assert_eq!(body["mode"], "DROPOFF");
        assert_eq!(body["startStep"], 2);
        assert_eq!(body["endStep"], 3);
        assert_eq!(body["limit"], 10);
        assert!(body.get("groupBy").is_none());
        assert_eq!(body["query"]["toTimestamp"], RANGE.1);
    }

    #[test]
    fn funnel_body_without_query_or_steps_fails() {
        let input = QueryInput::default();
        assert!(build_query_body(QueryKind::Funnel, Some(json!({})), &input, RANGE).is_err());
        assert!(build_query_body(
            QueryKind::MatchingUsers,
            Some(json!({"query": {"steps": []}})),
            &input,
            RANGE
        )
        .is_err());
        assert!(build_query_body(QueryKind::Views, Some(json!([])), &input, RANGE).is_err());
    }

    #[test]
    fn saved_funnel_becomes_a_query_request() {
        let saved = json!({
            "id": "21", "appName": "web-app", "name": "APM", "createdAt": "1783433258498",
            "steps": [{"conditions": []}], "filters": [{"field": "OS"}], "groupBy": "GROUP_BY_CX_USER_ID",
            "conversionWindowSeconds": "3600", "timeRange": {"relativeMinutes": "720"},
            "breakdown": {"field": "OS", "limit": 3}
        });
        assert_eq!(
            request_from_saved(QueryKind::Funnel, &saved),
            json!({
                "query": {"appNames": ["web-app"], "steps": [{"conditions": []}],
                          "filters": [{"field": "OS"}], "conversionWindowSeconds": "3600"},
                "groupBy": "GROUP_BY_CX_USER_ID",
                "breakdown": {"field": "OS", "limit": 3}
            })
        );
    }

    #[test]
    fn saved_pathway_and_heatmap_become_query_requests() {
        let pathway = json!({"id": "2", "appName": "web-app", "name": "p", "pageName": "explore",
                             "pagesCount": 5, "limit": 8, "timeRange": {"relativeMinutes": "60"}});
        assert_eq!(
            request_from_saved(QueryKind::Pathways, &pathway),
            json!({"appNames": ["web-app"], "pageName": "explore", "pagesCount": 5, "limit": 8})
        );
        let heatmap = json!({"id": "3", "appName": "web-app", "viewName": "alerts", "mode": "CLICK_MAP",
                             "snapshotId": "x", "actionNameFilter": "NAMED_ONLY"});
        assert_eq!(
            request_from_saved(QueryKind::Clicks, &heatmap),
            json!({"appName": "web-app", "pageName": "alerts", "actionNameFilter": "NAMED_ONLY"})
        );
        assert_eq!(
            request_from_saved(QueryKind::ScrollMap, &heatmap),
            json!({"appName": "web-app", "pageName": "alerts"})
        );
    }

    #[test]
    fn saved_time_ranges_resolve_to_seconds() {
        let now = 1_763_337_600 + 3_600;
        let midnight = 1_763_337_600;
        let fixed = json!({"timeRange": {"fixed": {"fromTimestamp": "100", "toTimestamp": 200}}});
        assert_eq!(resolve_saved_time_range(&fixed, now).unwrap(), (100, 200));
        let relative = json!({"timeRange": {"relativeMinutes": "1440"}});
        assert_eq!(
            resolve_saved_time_range(&relative, now).unwrap(),
            (now - 86_400, now)
        );
        let today = json!({"timeRange": {"calendar": "CALENDAR_WINDOW_TODAY"}});
        assert_eq!(
            resolve_saved_time_range(&today, now).unwrap(),
            (midnight, now)
        );
        let yesterday = json!({"timeRange": {"calendar": "CALENDAR_WINDOW_YESTERDAY"}});
        assert_eq!(
            resolve_saved_time_range(&yesterday, now).unwrap(),
            (midnight - 86_400, midnight)
        );
        assert!(resolve_saved_time_range(&json!({}), now).is_err());
        assert!(resolve_saved_time_range(&json!({"timeRange": {"fixed": {}}}), now).is_err());
    }

    #[test]
    fn update_strips_read_only_fields() {
        let body = strip_read_only_fields(
            json!({"id": "21", "appName": "web-app", "createdAt": "1", "name": "n", "steps": []}),
        )
        .unwrap();
        assert_eq!(body, json!({"name": "n", "steps": []}));
        assert!(strip_read_only_fields(json!([])).is_err());
    }

    #[test]
    fn display_enum_strips_known_prefixes() {
        assert_eq!(display_enum(Some("GROUP_BY_CX_USER_ID")), "CX_USER_ID");
        assert_eq!(display_enum(Some("TOP_ELEMENTS")), "TOP_ELEMENTS");
        assert_eq!(display_enum(None), "-");
    }
}
