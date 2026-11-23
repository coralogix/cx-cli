use std::collections::HashSet;

use opentelemetry::{Array, KeyValue, Value};
use opentelemetry_sdk::trace::{SpanData, SpanLimits};

pub(crate) const MAX_COMMAND_DEPTH: usize = 8;
pub(crate) const MAX_LABEL_BYTES: usize = 64;
pub(crate) const MAX_FLAGS: usize = 32;
pub(crate) const MAX_KEY_BYTES: usize = 128;
pub(crate) const MAX_VALUE_BYTES: usize = 576;
pub(crate) const MAX_ARRAY_ITEMS: usize = 32;
pub(crate) const MAX_SPAN_NAME_BYTES: usize = 640;
pub(crate) const MAX_TRACE_STATE_BYTES: usize = 512;
pub(crate) const MAX_ATTRIBUTES: usize = 64;
pub(crate) const MAX_EVENTS_OR_LINKS: usize = 8;
pub(crate) const MAX_SPANS: usize = 256;
pub(crate) const MAX_SPAN_BYTES: usize = 32 * 1024;
pub(crate) const MAX_BATCH_BYTES: usize = 512 * 1024;

const MAX_ATTRIBUTE_BYTES: usize = 3 * 1024;
const MAX_SPAN_ATTRIBUTE_BYTES: usize = 20 * 1024;
const MAX_RESOURCE_ATTRIBUTE_BYTES: usize = 16 * 1024;

pub(crate) const SDK_LIMITS: SpanLimits = SpanLimits {
    max_events_per_span: MAX_EVENTS_OR_LINKS as u32,
    max_attributes_per_span: MAX_ATTRIBUTES as u32,
    max_links_per_span: MAX_EVENTS_OR_LINKS as u32,
    max_attributes_per_event: MAX_ATTRIBUTES as u32,
    max_attributes_per_link: MAX_ATTRIBUTES as u32,
};

pub(crate) fn bounded_text(value: &str, bytes: usize) -> bool {
    value.len() <= bytes && !value.chars().any(char::is_control)
}

pub(crate) fn label(value: &str) -> bool {
    !value.is_empty()
        && bounded_text(value, MAX_LABEL_BYTES)
        && !value.chars().any(char::is_whitespace)
        && !value.contains(['=', '/', '\\'])
}

pub(crate) fn flag(value: &str) -> bool {
    value.strip_prefix("--").is_some_and(label)
        || value
            .strip_prefix('-')
            .is_some_and(|short| short.chars().count() == 1 && short != "-" && label(short))
}

pub(crate) fn valid_attribute(attribute: &KeyValue) -> bool {
    attribute_size(attribute).is_some()
}

fn attribute_size(attribute: &KeyValue) -> Option<usize> {
    let key = attribute.key.as_str();
    if key.is_empty() || !bounded_text(key, MAX_KEY_BYTES) {
        return None;
    }

    let size = match &attribute.value {
        Value::Bool(_) | Value::I64(_) => 16,
        Value::F64(value) if value.is_finite() => 16,
        Value::String(value) if bounded_text(value.as_str(), MAX_VALUE_BYTES) => {
            value.as_str().len()
        }
        Value::Array(Array::Bool(values)) if values.len() <= MAX_ARRAY_ITEMS => values.len() * 16,
        Value::Array(Array::I64(values)) if values.len() <= MAX_ARRAY_ITEMS => values.len() * 16,
        Value::Array(Array::F64(values))
            if values.len() <= MAX_ARRAY_ITEMS && values.iter().all(|value| value.is_finite()) =>
        {
            values.len() * 16
        }
        Value::Array(Array::String(values)) if values.len() <= MAX_ARRAY_ITEMS => {
            let mut bytes = 0;
            for value in values {
                if !bounded_text(value.as_str(), MAX_VALUE_BYTES) {
                    return None;
                }
                bytes += value.as_str().len() + 16;
            }
            bytes
        }
        _ => return None,
    };
    let total = key.len() + size + 16;
    (total <= MAX_ATTRIBUTE_BYTES).then_some(total)
}

pub(crate) fn bound_resource_attributes(attributes: &mut Vec<KeyValue>) {
    let mut remaining = MAX_RESOURCE_ATTRIBUTE_BYTES;
    // Reserve the service name and version.
    retain_attributes(attributes, MAX_ATTRIBUTES - 2, &mut remaining);
}

pub(crate) fn bound_initial_child_attributes(attributes: &mut Vec<KeyValue>) {
    // Leave room for completion fields such as HTTP status and error type.
    const RESERVED_ATTRIBUTES: usize = 2;
    let mut remaining = MAX_SPAN_ATTRIBUTE_BYTES - RESERVED_ATTRIBUTES * MAX_ATTRIBUTE_BYTES;
    retain_attributes(
        attributes,
        MAX_ATTRIBUTES - RESERVED_ATTRIBUTES,
        &mut remaining,
    );
}

fn retain_attributes(attributes: &mut Vec<KeyValue>, limit: usize, remaining: &mut usize) -> u32 {
    let mut keys = HashSet::new();
    let previous_len = attributes.len();
    attributes.retain(|attribute| {
        if keys.len() == limit {
            return false;
        }
        let Some(size) = attribute_size(attribute) else {
            return false;
        };
        if size > *remaining || !keys.insert(attribute.key.clone()) {
            return false;
        }
        *remaining -= size;
        true
    });
    (previous_len - attributes.len()) as u32
}

pub(crate) fn bound_span(span: &mut SpanData) {
    if span.name.is_empty() || !bounded_text(&span.name, MAX_SPAN_NAME_BYTES) {
        span.name = "operation".into();
    }
    if matches!(span.status, opentelemetry::trace::Status::Error { .. }) {
        // Status descriptions commonly contain arbitrary application errors.
        span.status = opentelemetry::trace::Status::error("");
    }

    let mut remaining = MAX_SPAN_ATTRIBUTE_BYTES;
    span.dropped_attributes_count +=
        retain_attributes(&mut span.attributes, MAX_ATTRIBUTES, &mut remaining);

    let event_count = span.events.events.len();
    span.events.events.retain_mut(|event| {
        if event.name.is_empty() || !bounded_text(&event.name, MAX_LABEL_BYTES) {
            return false;
        }
        event.dropped_attributes_count +=
            retain_attributes(&mut event.attributes, MAX_ATTRIBUTES, &mut remaining);
        true
    });
    span.events.dropped_count += (event_count - span.events.events.len()) as u32;

    let link_count = span.links.links.len();
    span.links.links.retain_mut(|link| {
        if !link.span_context.is_valid()
            || !bounded_text(
                &link.span_context.trace_state().header(),
                MAX_TRACE_STATE_BYTES,
            )
        {
            return false;
        }
        link.dropped_attributes_count +=
            retain_attributes(&mut link.attributes, MAX_ATTRIBUTES, &mut remaining);
        true
    });
    span.links.dropped_count += (link_count - span.links.links.len()) as u32;
}
