use anyhow::{bail, Result};
use chrono::{DateTime, Utc};

/// Parse a time expression into the exact UTC timestamp format expected by the
/// Dataprime API (`2006-01-02T15:04:05.000Z`).
///
/// Accepted forms:
/// - `now`               → current UTC time
/// - `now-1h`            → relative: subtract a duration
/// - `now - 3d`          → spaces around the minus are fine
/// - ISO-8601            → `2024-01-01T00:00:00Z` or any RFC3339 variant
///
/// Duration tokens are powered by [`humantime`] and support `s`, `m`, `h`,
/// `d`, `w` and compound forms like `1h30m`.
pub fn parse_timestamp(input: &str) -> Result<String> {
    let trimmed = input.trim();

    let dt: DateTime<Utc> = if trimmed.eq_ignore_ascii_case("now") {
        Utc::now()
    } else if let Some(rest) = trimmed.strip_prefix("now") {
        let rest = rest.trim();
        if let Some(duration_str) = rest.strip_prefix('-') {
            let duration_str = duration_str.trim();
            let std_duration = humantime::parse_duration(duration_str).map_err(|e| {
                anyhow::anyhow!(
                    "Could not parse duration '{}' in '{}': {e}. \
                     Examples: '1h', '30m', '3d', '1w'.",
                    duration_str,
                    input
                )
            })?;
            let chrono_duration = chrono::Duration::from_std(std_duration)?;
            Utc::now() - chrono_duration
        } else {
            bail!(
                "Invalid time expression '{}'. \
                 Use 'now', 'now-1h', 'now - 3d', or an ISO-8601 timestamp.",
                input
            );
        }
    } else {
        trimmed.parse::<DateTime<Utc>>().map_err(|_| {
            anyhow::anyhow!(
                "Could not parse time '{}'. \
                 Use 'now', 'now-1h', 'now - 3d', or ISO-8601 (e.g. '2024-01-01T00:00:00Z').",
                input
            )
        })?
    };

    Ok(format_api_timestamp(dt))
}

/// Format a UTC instant as the API's exact timestamp string.
/// Example output: `2024-01-01T12:00:00.000Z`
fn format_api_timestamp(dt: DateTime<Utc>) -> String {
    dt.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn now_produces_a_timestamp() {
        let ts = parse_timestamp("now").unwrap();
        assert!(ts.ends_with('Z'));
        assert!(ts.contains('T'));
    }

    #[test]
    fn now_with_spaces_works() {
        let ts = parse_timestamp("  now  ").unwrap();
        assert!(ts.ends_with('Z'));
    }

    #[test]
    fn relative_compact() {
        let ts = parse_timestamp("now-1h").unwrap();
        assert!(ts.ends_with('Z'));
    }

    #[test]
    fn relative_with_spaces() {
        let ts = parse_timestamp("now - 3d").unwrap();
        assert!(ts.ends_with('Z'));
    }

    #[test]
    fn iso8601_passthrough() {
        let ts = parse_timestamp("2024-01-01T00:00:00Z").unwrap();
        assert_eq!(ts, "2024-01-01T00:00:00.000Z");
    }

    #[test]
    fn invalid_expression_errors() {
        assert!(parse_timestamp("yesterday").is_err());
        assert!(parse_timestamp("now+1h").is_err());
    }
}
