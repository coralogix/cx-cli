use std::collections::BTreeMap;

use anyhow::{bail, Result as AnyResult};
use serde::{Deserialize, Deserializer};
use serde_json::{json, Value};

use crate::api_client::CxClient;
use crate::error::Result;

const SERVICES_PATH: &str = "/profiles/services/v1/overview-by-category";
const METADATA_PATH: &str = "/profiles/services/v1/metadata";
const FILTER_KEYS_PATH: &str = "/profiles/filters/v1/keys";
const FILTERS_PATH: &str = "/profiles/filters/v1";
const METRICS_PATH: &str = "/profiles/metrics/v1";
const FLAME_GRAPH_PATH: &str = "/profiles/flame-graph/v1";
const MAX_FLAME_GRAPH_JSON_DEPTH: usize = 1000;

pub const ENVIRONMENT_FILTER_KEY: &str = "deployment.environment.name";

const PROFILE_TYPES: &[&str] = &[
    "CPU",
    "WALL",
    "MEMORY_ALLOCATION_BYTES",
    "MEMORY_ALLOCATION_OBJECTS",
    "MEMORY_HEAP_BYTES",
    "MEMORY_HEAP_OBJECTS",
];

pub fn parse_profile_type(input: &str) -> AnyResult<&'static str> {
    let normalized = input.trim().to_uppercase().replace('-', "_");
    match PROFILE_TYPES.iter().find(|name| **name == normalized) {
        Some(name) => Ok(name),
        None => bail!(
            "unknown profile type '{input}'; allowed: cpu, wall, memory-allocation-bytes, \
             memory-allocation-objects, memory-heap-bytes, memory-heap-objects"
        ),
    }
}

pub fn profile_type_unit(profile_type: &str) -> &'static str {
    match profile_type {
        "CPU" | "WALL" => "seconds",
        "MEMORY_ALLOCATION_BYTES" | "MEMORY_HEAP_BYTES" => "bytes",
        _ => "objects",
    }
}

pub fn parse_category(input: &str) -> AnyResult<&'static str> {
    match input.trim().to_lowercase().as_str() {
        "cpu" => Ok("PROFILE_CATEGORY_CPU"),
        "memory" => Ok("PROFILE_CATEGORY_MEMORY"),
        _ => bail!("unknown category '{input}'; allowed: cpu, memory"),
    }
}

fn int64<'de, D>(deserializer: D) -> std::result::Result<i64, D::Error>
where
    D: Deserializer<'de>,
{
    match Value::deserialize(deserializer)? {
        Value::Number(n) => n
            .as_i64()
            .ok_or_else(|| serde::de::Error::custom(format!("expected an int64, got {n}"))),
        Value::String(s) => s
            .parse()
            .map_err(|_| serde::de::Error::custom(format!("expected an int64, got '{s}'"))),
        other => Err(serde::de::Error::custom(format!(
            "expected an int64, got {other}"
        ))),
    }
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TimeSample {
    #[serde(default, deserialize_with = "int64")]
    pub timestamp: i64,
    #[serde(default)]
    pub value: f64,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CpuData {
    #[serde(default)]
    pub series: Vec<TimeSample>,
    #[serde(default)]
    pub total_cpu_seconds: f64,
    #[serde(default)]
    pub samples_per_sec: f64,
    #[serde(default)]
    pub wall_series: Vec<TimeSample>,
    #[serde(default)]
    pub total_wall_seconds: f64,
    #[serde(default)]
    pub wall_samples_per_second: f64,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MemoryData {
    #[serde(default)]
    pub series: Vec<TimeSample>,
    #[serde(default)]
    pub total_memory_allocations_count: f64,
    #[serde(default, deserialize_with = "int64")]
    pub total_memory_allocations_bytes: i64,
    #[serde(default)]
    pub heap_bytes_series: Vec<TimeSample>,
    #[serde(default, deserialize_with = "int64")]
    pub total_heap_bytes: i64,
    #[serde(default)]
    pub total_heap_objects: f64,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ServiceOverview {
    #[serde(default)]
    pub service_name: String,
    #[serde(default, deserialize_with = "int64")]
    pub last_active: i64,
    pub language: Option<String>,
    pub cpu: Option<CpuData>,
    pub memory: Option<MemoryData>,
}

#[derive(Debug, Deserialize, Default)]
pub struct ServiceOverviewResponse {
    #[serde(default)]
    pub services: Vec<ServiceOverview>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileTypeInfo {
    #[serde(default)]
    pub profile_type: String,
    #[serde(default)]
    pub has_data: bool,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ServiceMetadata {
    pub language: Option<String>,
    #[serde(default)]
    pub profile_types: Vec<ProfileTypeInfo>,
}

#[derive(Debug, Deserialize, Default)]
pub struct ServiceMetadataResponse {
    pub service: Option<ServiceMetadata>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FilterKeysResponse {
    #[serde(default)]
    pub filter_keys: Vec<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct FilterValues {
    #[serde(default)]
    pub values: Vec<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct FiltersResponse {
    #[serde(default)]
    pub filters: BTreeMap<String, FilterValues>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TimePoint {
    #[serde(default, deserialize_with = "int64")]
    pub timestamp: i64,
    #[serde(default)]
    pub avg_value: f64,
    #[serde(default)]
    pub total_value: f64,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MetricSeries {
    #[serde(default)]
    pub group_by_value: String,
    #[serde(default)]
    pub points: Vec<TimePoint>,
}

#[derive(Debug, Deserialize, Default)]
pub struct MetricsResponse {
    #[serde(default)]
    pub series: Vec<MetricSeries>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LineIndexed {
    #[serde(default)]
    pub function_filename_index: usize,
    pub function_name_index: Option<usize>,
    pub function_system_name_index: Option<usize>,
    pub package_name_index: Option<usize>,
    pub line: Option<Value>,
    #[serde(default)]
    pub classification: String,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FlameGraphNode {
    #[serde(default)]
    pub address: String,
    #[serde(default)]
    pub function_locations: Vec<LineIndexed>,
    #[serde(default)]
    pub frame_type: String,
    #[serde(default)]
    pub total_value: f64,
    #[serde(default)]
    pub self_value: f64,
    #[serde(default)]
    pub children: Vec<FlameGraphNode>,
}

#[derive(Debug, Deserialize, Default)]
pub struct StringTable {
    #[serde(default)]
    pub strings: String,
    #[serde(default)]
    pub offset: Vec<usize>,
    #[serde(default)]
    pub length: Vec<usize>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FlameGraphResponse {
    pub flamegraph: Option<FlameGraphNode>,
    pub string_table: Option<StringTable>,
    #[serde(default)]
    pub warn_messages: Vec<Value>,
    #[serde(default)]
    pub was_seen: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProfilingFilter {
    pub key: String,
    pub values: Vec<String>,
    pub predicate: &'static str,
}

pub struct ServiceQuery<'p> {
    pub service_name: &'p str,
    pub profile_type: &'p str,
    pub start: i64,
    pub end: i64,
    pub filters: &'p [ProfilingFilter],
}

pub struct ServicesParams<'p> {
    pub category: &'p str,
    pub start: i64,
    pub end: i64,
    pub environments: &'p [String],
    pub limit: u32,
    pub offset: u32,
}

fn date_range_body(start: i64, end: i64) -> Value {
    json!({
        "startTime": start.to_string(),
        "endTime": end.to_string(),
        "timeUnit": "SECONDS",
    })
}

fn filters_body(filters: &[ProfilingFilter]) -> Value {
    filters
        .iter()
        .map(|f| json!({ "key": f.key, "value": f.values, "predicate": f.predicate }))
        .collect()
}

fn service_query_body(query: &ServiceQuery<'_>) -> Value {
    json!({
        "serviceName": query.service_name,
        "profileType": query.profile_type,
        "dateRange": date_range_body(query.start, query.end),
    })
}

fn services_body(params: &ServicesParams<'_>) -> Value {
    let mut filters = Vec::new();
    if !params.environments.is_empty() {
        filters.push(ProfilingFilter {
            key: ENVIRONMENT_FILTER_KEY.to_string(),
            values: params.environments.to_vec(),
            predicate: "EQUAL",
        });
    }
    json!({
        "dateRange": date_range_body(params.start, params.end),
        "category": params.category,
        "filters": filters_body(&filters),
        "limit": params.limit,
        "offset": params.offset,
    })
}

pub struct ProfilingApi<'a> {
    client: &'a CxClient,
}

impl<'a> ProfilingApi<'a> {
    pub fn new(client: &'a CxClient) -> Self {
        Self { client }
    }

    pub async fn services(&self, params: &ServicesParams<'_>) -> Result<ServiceOverviewResponse> {
        self.client
            .post(SERVICES_PATH, &services_body(params))
            .await
    }

    pub async fn service_metadata(
        &self,
        service_name: &str,
        start: i64,
        end: i64,
    ) -> Result<ServiceMetadataResponse> {
        let body = json!({
            "serviceName": service_name,
            "dateRange": date_range_body(start, end),
        });
        self.client.post(METADATA_PATH, &body).await
    }

    pub async fn filter_keys(&self, query: &ServiceQuery<'_>) -> Result<FilterKeysResponse> {
        self.client
            .post(FILTER_KEYS_PATH, &service_query_body(query))
            .await
    }

    pub async fn filters(&self, query: &ServiceQuery<'_>) -> Result<FiltersResponse> {
        let mut body = service_query_body(query);
        body["filters"] = filters_body(query.filters);
        self.client.post(FILTERS_PATH, &body).await
    }

    pub async fn metrics(
        &self,
        query: &ServiceQuery<'_>,
        sum_by: &str,
        max_buckets: u32,
    ) -> Result<MetricsResponse> {
        let mut body = service_query_body(query);
        body["filters"] = filters_body(query.filters);
        body["sumBy"] = json!(sum_by);
        body["maxBuckets"] = json!(max_buckets);
        self.client.post(METRICS_PATH, &body).await
    }

    pub async fn flame_graph(&self, query: &ServiceQuery<'_>, show_kernel: bool) -> Result<String> {
        let mut body = service_query_body(query);
        body["filters"] = filters_body(query.filters);
        body["showKernel"] = json!(show_kernel);
        self.client.post_raw(FLAME_GRAPH_PATH, &body).await
    }
}

pub fn parse_flame_graph(text: &str) -> Result<FlameGraphResponse> {
    let mut depth = 0usize;
    let mut max_depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for byte in text.bytes() {
        if in_string {
            match byte {
                _ if escaped => escaped = false,
                b'\\' => escaped = true,
                b'"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'{' | b'[' => {
                depth += 1;
                max_depth = max_depth.max(depth);
            }
            b'}' | b']' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    if max_depth > MAX_FLAME_GRAPH_JSON_DEPTH {
        return Err(<serde_json::Error as serde::de::Error>::custom(format!(
            "flame graph nests {max_depth} levels deep, more than the supported {MAX_FLAME_GRAPH_JSON_DEPTH}; narrow the time range or add filters"
        ))
        .into());
    }
    let mut deserializer = serde_json::Deserializer::from_str(text);
    deserializer.disable_recursion_limit();
    let response = FlameGraphResponse::deserialize(&mut deserializer)?;
    deserializer.end()?;
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_profile_type_accepts_short_and_proto_forms() {
        assert_eq!(parse_profile_type("cpu").unwrap(), "CPU");
        assert_eq!(
            parse_profile_type("memory-heap-bytes").unwrap(),
            "MEMORY_HEAP_BYTES"
        );
        assert_eq!(
            parse_profile_type(" MEMORY_ALLOCATION_OBJECTS ").unwrap(),
            "MEMORY_ALLOCATION_OBJECTS"
        );
    }

    #[test]
    fn parse_profile_type_rejects_off_cpu_and_unknown() {
        assert!(parse_profile_type("OFF_CPU").is_err());
        let err = parse_profile_type("heap").unwrap_err();
        assert!(err.to_string().contains("unknown profile type 'heap'"));
    }

    #[test]
    fn profile_type_unit_maps_every_profile_type() {
        assert_eq!(profile_type_unit("CPU"), "seconds");
        assert_eq!(profile_type_unit("WALL"), "seconds");
        assert_eq!(profile_type_unit("MEMORY_ALLOCATION_BYTES"), "bytes");
        assert_eq!(profile_type_unit("MEMORY_HEAP_OBJECTS"), "objects");
    }

    #[test]
    fn parse_category_accepts_cpu_and_memory() {
        assert_eq!(parse_category("CPU").unwrap(), "PROFILE_CATEGORY_CPU");
        assert_eq!(parse_category("memory").unwrap(), "PROFILE_CATEGORY_MEMORY");
        assert!(parse_category("disk").is_err());
    }

    #[test]
    fn deserialize_service_overview_with_int64_strings() {
        let resp: ServiceOverviewResponse = serde_json::from_value(json!({
            "services": [{
                "serviceName": "checkout",
                "lastActive": "1792588680",
                "language": "rust",
                "memory": {
                    "series": [{ "timestamp": "1792585254", "value": 1.5 }],
                    "totalMemoryAllocationsCount": 3.0,
                    "totalMemoryAllocationsBytes": "4096",
                    "heapBytesSeries": [],
                    "totalHeapBytes": "2048",
                    "totalHeapObjects": 7.0
                }
            }]
        }))
        .unwrap();
        let service = &resp.services[0];
        assert_eq!(service.last_active, 1792588680);
        let memory = service.memory.as_ref().unwrap();
        assert_eq!(memory.total_memory_allocations_bytes, 4096);
        assert_eq!(memory.total_heap_bytes, 2048);
        assert_eq!(memory.series[0].timestamp, 1792585254);
        assert!(service.cpu.is_none());
    }

    #[test]
    fn deserialize_rejects_non_numeric_int64() {
        let result: std::result::Result<TimeSample, _> =
            serde_json::from_value(json!({ "timestamp": "soon" }));
        assert!(result.is_err());
    }

    #[test]
    fn deserialize_service_without_language() {
        let resp: ServiceOverviewResponse = serde_json::from_value(json!({
            "services": [{ "serviceName": "kafka", "lastActive": "1", "cpu": {} }]
        }))
        .unwrap();
        assert!(resp.services[0].language.is_none());
        assert!(resp.services[0].cpu.is_some());
    }

    #[test]
    fn deserialize_service_metadata() {
        let resp: ServiceMetadataResponse = serde_json::from_value(json!({
            "service": {
                "language": "rust",
                "profileTypes": [
                    { "profileType": "CPU", "hasData": true },
                    { "profileType": "WALL", "hasData": false }
                ]
            }
        }))
        .unwrap();
        let service = resp.service.unwrap();
        assert_eq!(service.profile_types.len(), 2);
        assert!(service.profile_types[0].has_data);
    }

    #[test]
    fn deserialize_filters_and_keys() {
        let keys: FilterKeysResponse =
            serde_json::from_value(json!({ "filterKeys": ["k8s.pod.name"] })).unwrap();
        assert_eq!(keys.filter_keys, vec!["k8s.pod.name"]);
        let filters: FiltersResponse = serde_json::from_value(json!({
            "filters": { "k8s.pod.name": { "values": ["a", "b"] } }
        }))
        .unwrap();
        assert_eq!(filters.filters["k8s.pod.name"].values, vec!["a", "b"]);
    }

    #[test]
    fn deserialize_metrics() {
        let resp: MetricsResponse = serde_json::from_value(json!({
            "series": [{
                "groupByValue": "pod-a",
                "points": [{ "timestamp": "10", "avgValue": 0.5, "totalValue": 30.0 }]
            }]
        }))
        .unwrap();
        assert_eq!(resp.series[0].group_by_value, "pod-a");
        assert_eq!(resp.series[0].points[0].timestamp, 10);
        assert_eq!(resp.series[0].points[0].total_value, 30.0);
    }

    #[test]
    fn deserialize_flame_graph() {
        let resp: FlameGraphResponse = serde_json::from_value(json!({
            "flamegraph": {
                "address": "",
                "functionLocations": [{
                    "functionFilenameIndex": 0,
                    "functionNameIndex": 1,
                    "classification": "FRAME_CLASSIFICATION_UNSPECIFIED"
                }],
                "frameType": "FRAME_TYPE_UNSPECIFIED",
                "totalValue": 10.0,
                "selfValue": 0.0,
                "children": []
            },
            "stringTable": { "strings": "cm9vdA==", "offset": [0, 0], "length": [0, 4] },
            "warnMessages": [{ "maxBytes": { "value": "100" } }],
            "wasSeen": true
        }))
        .unwrap();
        let root = resp.flamegraph.unwrap();
        assert_eq!(root.function_locations[0].function_name_index, Some(1));
        assert_eq!(root.function_locations[0].function_system_name_index, None);
        assert_eq!(resp.string_table.unwrap().length, vec![0, 4]);
        assert_eq!(resp.warn_messages.len(), 1);
        assert!(resp.was_seen);
    }

    #[test]
    fn services_body_always_sends_limit_and_environment_filter() {
        let environments = vec!["prod".to_string()];
        let body = services_body(&ServicesParams {
            category: "PROFILE_CATEGORY_CPU",
            start: 100,
            end: 200,
            environments: &environments,
            limit: 20,
            offset: 40,
        });
        assert_eq!(
            body["dateRange"],
            json!({ "startTime": "100", "endTime": "200", "timeUnit": "SECONDS" })
        );
        assert_eq!(body["category"], "PROFILE_CATEGORY_CPU");
        assert_eq!(body["limit"], 20);
        assert_eq!(body["offset"], 40);
        assert_eq!(
            body["filters"],
            json!([{ "key": ENVIRONMENT_FILTER_KEY, "value": ["prod"], "predicate": "EQUAL" }])
        );
    }

    #[test]
    fn service_query_body_omits_filters() {
        let filters = vec![ProfilingFilter {
            key: "k8s.pod.name".to_string(),
            values: vec!["a".to_string()],
            predicate: "EQUAL",
        }];
        let body = service_query_body(&ServiceQuery {
            service_name: "checkout",
            profile_type: "CPU",
            start: 1,
            end: 2,
            filters: &filters,
        });
        assert_eq!(body["serviceName"], "checkout");
        assert_eq!(body["profileType"], "CPU");
        assert!(body.get("filters").is_none());
    }
}
