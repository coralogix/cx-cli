//! Semantic field and metric search via the Coralogix Semantic Search HTTP API.
pub use crate::api::semantic_search::{
    semantic_field_lookup, semantic_metric_lookup, semantic_search_gateway_from_api_endpoint,
    semantic_search_post, SemanticFieldResult, SemanticMetricResult,
};
