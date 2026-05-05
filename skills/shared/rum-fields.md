# RUM Field Reference

All fields are under `$d.cx_rum.*`.

## Table of Contents

- [Event Context](#event-context)
- [Error Context & Grouping](#error-context--grouping)
- [Session Context](#session-context)
- [Version & Environment](#version--environment)
- [Page Context](#page-context)
- [Network Request Context](#network-request-context)
- [Web Vitals Context](#web-vitals-context)
- [Interaction Context](#interaction-context)
- [Resource Context](#resource-context)
- [Mobile Contexts](#mobile-contexts)
- [Other Fields](#other-fields)

---

## Event Context

`event_context.*`

| Field | Description |
|-------|-------------|
| `type` | Event type (`error`, `resources`, `network-request`, `user-interaction`, `web-vitals`, `longtask`, `life-cycle`, `dom`, `log`, `custom-measurement`, `mobile-vitals`) |
| `severity` | Severity level (`5` = error, applies to all event types) |
| `source` | Event source |

## Error Context & Grouping

`error_context.*`

| Field | Description |
|-------|-------------|
| `error_message` | Error message text |
| `error_type` | Error classification |
| `is_crash` | Whether the error is a crash |
| `original_stacktrace` | Stack trace |
| `threads` | Thread information |

Top-level error grouping fields:

| Field | Description |
|-------|-------------|
| `rum_template_id` | Error fingerprint - groups similar errors into distinct issues |
| `fingerPrint` | Additional fingerprint field |

## Session Context

`session_context.*`

| Field | Description |
|-------|-------------|
| `user_id`, `user_email`, `user_name`, `user_metadata` | User identity |
| `session_id`, `session_creation_date` | Session identity |
| `browser`, `browserVersion` | Browser info |
| `os`, `osVersion`, `device`, `user_agent` | Device info |
| `ip` | IP address |
| `ip_geoip.country_name`, `city_name`, `continent_name`, `is_local` | Geolocation |
| `hasRecording`, `hasScreenshot`, `hasError` | Session flags |

## Version & Environment

| Field | Description |
|-------|-------------|
| `version_metadata.app_name` | Application name (use for RUM app filtering) |
| `version_metadata.app_version` | Application version |
| `platform` | Platform (web, iOS, Android) |
| `environment` | Deployment environment |
| `labels.*` | Custom labels (e.g. `labels.mfeApp`, `labels.mfeVersion`) |

## Page Context

`page_context.*`

| Field | Description |
|-------|-------------|
| `page_url` | Full page URL |
| `page_fragments` | URL path - **always use this for groupby** |
| `referrer` | Referring page |
| `page_url_blueprint` | URL pattern/template |

## Network Request Context

`network_request_context.*`

| Field | Description |
|-------|-------------|
| `url`, `url_blueprint`, `fragments`, `host`, `schema` | Request URL parts |
| `method` | HTTP method |
| `status_code`, `status_text` | Response status |
| `duration` | Request duration |
| `response_content_length` | Response size |
| `source` | Request source |

## Web Vitals Context

`web_vitals_context.*`

| Field | Description |
|-------|-------------|
| `name` | Vital name: `LT` (Load Time), `LCP`, `FID`, `CLS`, `FCP`, `INP`, `TTFB`, `TBT` |
| `value` | Metric value |
| `rating` | Rating classification |
| `domComplete`, `domInteractive` | DOM timing milestones |
| `domContentLoadedEventStart`, `domContentLoadedEventEnd` | DCL timing |
| `loadEventStart`, `loadEventEnd` | Load event timing |
| `attribution.element`, `attribution.eventTarget` | Attribution fields |

## Interaction Context

`interaction_context.*`

| Field | Description |
|-------|-------------|
| `event_name` | Interaction event type |
| `target_element` | HTML element tag |
| `target_element_inner_text` | User-visible button/link text - **use this for groupby** |
| `target_element_type` | Element type |
| `element_id`, `element_classes` | Element identifiers |

## Resource Context

`resource_context.*`

| Field | Description |
|-------|-------------|
| `initiatorType` | Resource type (script, img, css, etc.) |
| `name`, `fragments` | Resource URL |
| `duration`, `responseStatus` | Loading performance |
| `transferSize`, `decodedBodySize` | Size metrics |
| `contentType`, `contentEncoding` | Content metadata |
| `deliveryType`, `nextHopProtocol` | Delivery info |

## Mobile Contexts

**Device Context** (`device_context.*`): `device`, `device_name`, `os`, `osVersion`, `emulator`

**Mobile SDK** (`mobile_sdk.*`): `framework`, `sdk_version`

**View Context** (`view_context.*`): `view`, `view_activity`, `view_fragment`

**Mobile Vitals** (`mobile_vitals_context.*`):
- CPU: `cpu.cpu_usage`, `cpu.total_cpu_time`, `cpu.main_thread_cpu_time`
- Memory: `memory.memory_utilization`, `memory.heap_max`, `memory.heap_used`
- Performance: `fps`, `cold`, `warm`, `slow_frozen.slow_frames`, `slow_frozen.frozen_frames`, `anr`

## Other Fields

| Field | Description |
|-------|-------------|
| `traceId`, `spanId` | Distributed tracing correlation |
| `screenshot_context.id`, `screenshotId` | Screenshot references |
| `log_context.message` | Console log message |
| `longtask_context.id`, `name`, `duration` | Long task details |
| `custom_measurement_context.name`, `value` | Custom metrics |
| `browser_sdk.version` | Browser SDK version |
