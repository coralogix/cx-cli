use std::collections::HashSet;
use std::io;

use opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceRequest;
use opentelemetry_proto::tonic::common::v1::{any_value::Value, KeyValue};
use opentelemetry_proto::tonic::trace::v1::{span, status, Span};
use prost::Message;

use super::limits::{
    bounded_text, MAX_ARRAY_ITEMS, MAX_ATTRIBUTES, MAX_BATCH_BYTES, MAX_EVENTS_OR_LINKS,
    MAX_KEY_BYTES, MAX_LABEL_BYTES, MAX_SPANS, MAX_SPAN_BYTES, MAX_SPAN_NAME_BYTES,
    MAX_TRACE_STATE_BYTES, MAX_VALUE_BYTES,
};

/// Fit the complete request to a single transport payload, dropping old spans
/// first. The CLI scope contains the execution span last on normal shutdown.
pub(crate) fn encode(mut request: ExportTraceServiceRequest) -> Option<Vec<u8>> {
    for scope in request
        .resource_spans
        .iter_mut()
        .flat_map(|r| &mut r.scope_spans)
    {
        scope.spans.retain(valid_span);
    }
    let mut to_drop = request.encoded_len().saturating_sub(MAX_BATCH_BYTES);
    if to_drop > 0 {
        for scope in request
            .resource_spans
            .iter_mut()
            .flat_map(|r| &mut r.scope_spans)
        {
            let mut count = 0;
            for span in &scope.spans {
                if to_drop == 0 {
                    break;
                }
                // The field key and length prefix also disappear on removal.
                to_drop = to_drop.saturating_sub(span.encoded_len());
                count += 1;
            }
            scope.spans.drain(..count);
        }
    }
    valid_request(&request).then(|| request.encode_to_vec())
}

pub(crate) fn validate_and_encode(payload: &[u8]) -> io::Result<Vec<u8>> {
    if payload.is_empty() || payload.len() > MAX_BATCH_BYTES {
        return Err(invalid_batch());
    }
    let request = ExportTraceServiceRequest::decode(payload).map_err(|_| invalid_batch())?;
    if !valid_request(&request) {
        return Err(invalid_batch());
    }
    Ok(request.encode_to_vec())
}

fn invalid_batch() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid OTLP trace batch")
}

pub(crate) fn valid_request(request: &ExportTraceServiceRequest) -> bool {
    let mut count = 0;
    if request.resource_spans.is_empty() || request.resource_spans.len() > 16 {
        return false;
    }

    for resource in &request.resource_spans {
        if !bounded_text(&resource.schema_url, MAX_VALUE_BYTES)
            || resource.resource.as_ref().is_some_and(|resource| {
                !resource.entity_refs.is_empty() || !valid_attributes(&resource.attributes)
            })
            || resource.scope_spans.len() > 32
        {
            return false;
        }

        for scope in &resource.scope_spans {
            count += scope.spans.len();
            if count > MAX_SPANS
                || !bounded_text(&scope.schema_url, MAX_VALUE_BYTES)
                || scope.scope.as_ref().is_some_and(|scope| {
                    !bounded_text(&scope.name, MAX_SPAN_NAME_BYTES)
                        || !bounded_text(&scope.version, MAX_VALUE_BYTES)
                        || !valid_attributes(&scope.attributes)
                })
                || !scope.spans.iter().all(valid_span)
            {
                return false;
            }
        }
    }

    count != 0 && request.encoded_len() <= MAX_BATCH_BYTES
}

fn valid_span(span: &Span) -> bool {
    valid_id(&span.trace_id, 16)
        && valid_id(&span.span_id, 8)
        && (span.parent_span_id.is_empty() || valid_id(&span.parent_span_id, 8))
        && bounded_text(&span.trace_state, MAX_TRACE_STATE_BYTES)
        && !span.name.is_empty()
        && bounded_text(&span.name, MAX_SPAN_NAME_BYTES)
        && span::SpanKind::try_from(span.kind).is_ok()
        && span.start_time_unix_nano > 0
        && span.start_time_unix_nano <= span.end_time_unix_nano
        && valid_attributes(&span.attributes)
        && span.events.len() <= MAX_EVENTS_OR_LINKS
        && span.events.iter().all(|event| {
            !event.name.is_empty()
                && bounded_text(&event.name, MAX_LABEL_BYTES)
                && valid_attributes(&event.attributes)
        })
        && span.links.len() <= MAX_EVENTS_OR_LINKS
        && span.links.iter().all(|link| {
            valid_id(&link.trace_id, 16)
                && valid_id(&link.span_id, 8)
                && bounded_text(&link.trace_state, MAX_TRACE_STATE_BYTES)
                && valid_attributes(&link.attributes)
        })
        && span.status.as_ref().is_none_or(|status| {
            status::StatusCode::try_from(status.code).is_ok() && status.message.is_empty()
        })
        && span.encoded_len() <= MAX_SPAN_BYTES
}

fn valid_id(id: &[u8], len: usize) -> bool {
    id.len() == len && id.iter().any(|&byte| byte != 0)
}

fn valid_attributes(attributes: &[KeyValue]) -> bool {
    let mut keys = HashSet::new();
    attributes.len() <= MAX_ATTRIBUTES
        && attributes.iter().all(|attribute| {
            !attribute.key.is_empty()
                && bounded_text(&attribute.key, MAX_KEY_BYTES)
                && attribute.key_strindex == 0
                && keys.insert(attribute.key.as_str())
                && attribute
                    .value
                    .as_ref()
                    .and_then(|value| value.value.as_ref())
                    .is_some_and(valid_value)
        })
}

fn valid_value(value: &Value) -> bool {
    match value {
        Value::ArrayValue(array) => {
            array.values.len() <= MAX_ARRAY_ITEMS
                && array
                    .values
                    .iter()
                    .all(|item| item.value.as_ref().is_some_and(valid_scalar))
        }
        value => valid_scalar(value),
    }
}

fn valid_scalar(value: &Value) -> bool {
    match value {
        Value::StringValue(value) => bounded_text(value, MAX_VALUE_BYTES),
        Value::BoolValue(_) | Value::IntValue(_) => true,
        Value::DoubleValue(value) => value.is_finite(),
        _ => false,
    }
}
