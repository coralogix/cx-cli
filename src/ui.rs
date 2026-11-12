//! Shared building blocks for human-readable (`-o text`) output.
//!
//! Every command's text renderer composes the same few blocks so the whole
//! CLI reads as one tool:
//!
//! | Block        | Function                               | Looks like                              |
//! |--------------|----------------------------------------|-----------------------------------------|
//! | header       | [`format_header`] / [`print_header`]   | `checkout · errors · last 1h   1,284`   |
//! | table        | [`format_table`]                       | borderless, dim `UPPERCASE` headers     |
//! | bar chart    | [`format_bar_chart`]                   | `▄▆█` columns with y/x axes             |
//! | sparkline    | [`sparkline`]                          | `▁▂▃▅▇▆▂▁` in one table cell            |
//! | callout      | [`format_callout`] / [`print_callout`] | `✓ Likely cause: …`, `↗ Open in …`      |
//! | numbers      | [`fmt_count`] / [`fmt_compact`]        | `1,284`, `1.2k`                         |
//!
//! Colours come from one small palette ([`ERROR`], [`WARN`], [`OK`],
//! [`ENTITY`], [`LINK`]) so every command means the same thing by "red".
//! The `colored` crate already honours `NO_COLOR`, `CLICOLOR_FORCE` and
//! non-TTY stdout, so nothing here needs to check for a terminal.
//!
//! These blocks are for text output only. JSON and TOON output never pass
//! through this module - agents read those formats and must get raw data.

use colored::{Color, ColoredString, Colorize};
use tabled::builder::Builder;
use tabled::settings::object::{Columns, Rows};
use tabled::settings::{Alignment, Format, Padding, Style};
use terminal_size::{terminal_size, Width};

// ── Palette ──────────────────────────────────────────────────────────────────

/// Errors, failures, critical severity.
pub const ERROR: Color = Color::Red;
/// Warnings, degraded state, things to look at.
pub const WARN: Color = Color::Yellow;
/// Healthy, enabled, success.
pub const OK: Color = Color::Green;
/// Named entities: services, hosts, dashboards, users.
pub const ENTITY: Color = Color::Magenta;
/// URLs and other things the user can open.
pub const LINK: Color = Color::Cyan;

/// Width used when stdout is not a terminal (pipes, CI, tests).
pub const DEFAULT_WIDTH: usize = 100;

/// Widest line the header rule and charts will use, even on a huge terminal.
const MAX_LAYOUT_WIDTH: usize = 100;

/// Current terminal width, capped to [`MAX_LAYOUT_WIDTH`].
pub fn layout_width() -> usize {
    terminal_size()
        .map(|(Width(w), _)| w as usize)
        .unwrap_or(DEFAULT_WIDTH)
        .min(MAX_LAYOUT_WIDTH)
}

// ── Numbers ──────────────────────────────────────────────────────────────────

/// Format an integer with thousands separators: `1284` → `1,284`.
pub fn fmt_count(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// Format a number compactly for axis labels: `42` → `42`, `1200` → `1.2k`,
/// `3_400_000` → `3.4M`.
pub fn fmt_compact(v: f64) -> String {
    let abs = v.abs();
    let (scaled, suffix) = if abs >= 1e9 {
        (v / 1e9, "G")
    } else if abs >= 1e6 {
        (v / 1e6, "M")
    } else if abs >= 1e3 {
        (v / 1e3, "k")
    } else {
        (v, "")
    };
    if suffix.is_empty() {
        if (scaled - scaled.round()).abs() < 1e-9 {
            format!("{}", scaled.round() as i64)
        } else {
            format!("{scaled:.1}")
        }
    } else if (scaled - scaled.round()).abs() < 0.05 {
        format!("{}{suffix}", scaled.round() as i64)
    } else {
        format!("{scaled:.1}{suffix}")
    }
}

/// Number of terminal columns a string occupies, ignoring ANSI escape codes.
pub fn visible_width(s: &str) -> usize {
    let mut width = 0;
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            // Skip a CSI sequence: ESC '[' … final byte in 0x40..=0x7e.
            if chars.peek() == Some(&'[') {
                chars.next();
                for c in chars.by_ref() {
                    if ('\x40'..='\x7e').contains(&c) {
                        break;
                    }
                }
            }
            continue;
        }
        width += 1;
    }
    width
}

// ── Header ───────────────────────────────────────────────────────────────────

/// A small coloured tag shown at the right end of a header, e.g. `▲ 9.4×`.
pub struct Badge {
    pub text: String,
    pub color: Color,
}

impl Badge {
    pub fn new(text: impl Into<String>, color: Color) -> Self {
        Self {
            text: text.into(),
            color,
        }
    }
}

/// Build the two-line header block.
///
/// `left` parts are joined with a dim ` · `. `right` (already coloured if
/// desired) is pushed to the right margin, followed by the optional badge.
/// A dim rule line closes the block.
pub fn format_header(left: &[&str], right: Option<&str>, badge: Option<&Badge>) -> String {
    let width = layout_width();
    let sep = " · ".dimmed().to_string();
    let left_str = left
        .iter()
        .map(|p| p.bold().to_string())
        .collect::<Vec<_>>()
        .join(&sep);

    let mut right_str = right.unwrap_or_default().to_string();
    if let Some(b) = badge {
        if !right_str.is_empty() {
            right_str.push_str("  ");
        }
        right_str.push_str(
            &format!(" {} ", b.text)
                .on_color(b.color)
                .white()
                .to_string(),
        );
    }

    let gap = width
        .saturating_sub(visible_width(&left_str) + visible_width(&right_str))
        .max(2);
    let line = if right_str.is_empty() {
        left_str
    } else {
        format!("{left_str}{}{right_str}", " ".repeat(gap))
    };
    let rule = "─".repeat(width).dimmed().to_string();
    format!("{line}\n{rule}")
}

/// Print [`format_header`] to stdout.
pub fn print_header(left: &[&str], right: Option<&str>, badge: Option<&Badge>) {
    println!("{}", format_header(left, right, badge));
}

// ── Table ────────────────────────────────────────────────────────────────────

/// True when a cell looks like a number: `42`, `1,284`, `9.4×`, `87.5%`.
/// Empty cells and `-` are neutral and ignored.
fn numeric_cell(cell: &str) -> Option<bool> {
    let trimmed = cell.trim();
    if trimmed.is_empty() || trimmed == "-" {
        return None;
    }
    let cleaned: String = trimmed
        .chars()
        .filter(|c| !matches!(c, ',' | '%' | '×' | '_'))
        .collect();
    let cleaned = cleaned
        .strip_suffix("ms")
        .or_else(|| cleaned.strip_suffix('s'))
        .or_else(|| cleaned.strip_suffix('x'))
        .unwrap_or(&cleaned);
    Some(cleaned.parse::<f64>().is_ok())
}

/// Columns whose non-empty cells are all numeric (and at least one is).
fn numeric_columns(rows: &[Vec<String>], ncols: usize) -> Vec<usize> {
    (0..ncols)
        .filter(|&col| {
            let mut saw_number = false;
            for row in rows {
                match row.get(col).map(|c| numeric_cell(c)) {
                    Some(Some(true)) => saw_number = true,
                    Some(Some(false)) => return false,
                    _ => {}
                }
            }
            saw_number
        })
        .collect()
}

/// Build a borderless table: dim `UPPERCASE` headers, two spaces between
/// columns, numeric columns right-aligned, no trailing whitespace.
pub fn format_table(headers: &[String], rows: Vec<Vec<String>>) -> String {
    let ncols = headers.len();
    let numeric = numeric_columns(&rows, ncols);

    let mut builder = Builder::default();
    builder.push_record(headers.to_vec());
    for row in rows {
        builder.push_record(row);
    }

    let mut table = builder.build();
    table.with(Style::empty());
    table.with(Padding::new(0, 2, 0, 0));
    table.modify(
        Rows::first(),
        Format::content(|s: &str| s.to_uppercase().dimmed().to_string()),
    );
    for col in numeric {
        table.modify(Columns::one(col), Alignment::right());
    }

    table
        .to_string()
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
}

// ── Charts ───────────────────────────────────────────────────────────────────

const BLOCKS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// One character per value, scaled from 0 to the series maximum.
/// `NaN` renders as a space. An all-zero series renders as `▁▁▁`.
pub fn sparkline(values: &[f64]) -> String {
    let max = values
        .iter()
        .copied()
        .filter(|v| v.is_finite())
        .fold(0.0, f64::max);
    values
        .iter()
        .map(|&v| {
            if !v.is_finite() {
                ' '
            } else if max <= 0.0 {
                BLOCKS[0]
            } else {
                let idx = ((v / max) * 7.0).round() as usize;
                BLOCKS[idx.min(7)]
            }
        })
        .collect()
}

/// Sum `values` into at most `width` buckets so a long series fits the
/// terminal. Shorter series are returned unchanged.
pub fn downsample(values: &[f64], width: usize) -> Vec<f64> {
    if width == 0 || values.len() <= width {
        return values.to_vec();
    }
    let per_bucket = values.len().div_ceil(width);
    values
        .chunks(per_bucket)
        .map(|chunk| chunk.iter().copied().filter(|v| v.is_finite()).sum())
        .collect()
}

/// A vertical bar chart with a y-axis and labelled x-axis.
///
/// * `values`    - one bar per value; call [`downsample`] first if needed.
/// * `labels`    - `(value index, text)` pairs placed under that bar. Labels
///   that would overlap the previous one are skipped.
/// * `height`    - number of bar rows (3 or 4 reads well).
/// * `bar_width` - characters per bar (1 for dense, 4 to 6 for a wide chart).
/// * `color`     - colour for the bars; axes are always dim.
pub fn format_bar_chart(
    values: &[f64],
    labels: &[(usize, String)],
    height: usize,
    bar_width: usize,
    color: Option<Color>,
) -> String {
    let height = height.max(1);
    let bar_width = bar_width.max(1);
    let max = values
        .iter()
        .copied()
        .filter(|v| v.is_finite())
        .fold(0.0, f64::max);
    let scale_max = if max > 0.0 { max } else { 1.0 };

    let y_labels: Vec<String> = (1..=height)
        .rev()
        .map(|row| fmt_compact(scale_max * row as f64 / height as f64))
        .collect();
    let label_w = y_labels
        .iter()
        .map(|l| l.chars().count())
        .max()
        .unwrap_or(1)
        .max(1);

    let paint = |s: String| -> ColoredString {
        match color {
            Some(c) => s.color(c),
            None => s.normal(),
        }
    };

    let mut lines = Vec::with_capacity(height + 2);
    for (i, row) in (1..=height).rev().enumerate() {
        let bars: String = values
            .iter()
            .map(|&v| {
                if !v.is_finite() || v <= 0.0 {
                    return ' ';
                }
                let level = v / scale_max * height as f64;
                let floor = (row - 1) as f64;
                if level >= row as f64 {
                    '█'
                } else if level > floor {
                    let eighths = ((level - floor) * 8.0).ceil() as usize;
                    BLOCKS[eighths.clamp(1, 8) - 1]
                } else {
                    ' '
                }
            })
            .flat_map(|ch| std::iter::repeat_n(ch, bar_width))
            .collect();
        lines.push(format!(
            "{:>w$} {}{}",
            y_labels[i].dimmed(),
            "┤".dimmed(),
            paint(bars.trim_end().to_string()),
            w = label_w
        ));
    }

    let axis_len = (values.len() * bar_width).max(1);
    lines.push(format!(
        "{:>w$} {}",
        "0".dimmed(),
        format!("└{}", "─".repeat(axis_len)).dimmed(),
        w = label_w
    ));

    if !labels.is_empty() {
        let mut axis = vec![' '; label_w + 2 + axis_len + 8];
        let mut next_free = 0;
        for (idx, text) in labels {
            let start = label_w + 2 + idx * bar_width;
            if start < next_free {
                continue; // would overlap the previous label
            }
            for (j, ch) in text.chars().enumerate() {
                if start + j < axis.len() {
                    axis[start + j] = ch;
                }
            }
            next_free = start + text.chars().count() + 1;
        }
        let axis: String = axis.into_iter().collect();
        lines.push(axis.trim_end().dimmed().to_string());
    }

    lines.join("\n")
}

// ── Callouts ─────────────────────────────────────────────────────────────────

/// The kind of a one-line note printed under a table or chart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Callout {
    /// `✓` - a finding or likely cause.
    Finding,
    /// `!` - something incomplete or degraded.
    Warning,
    /// `↗` - something the user can open.
    Link,
    /// `·` - neutral information.
    Info,
}

/// Build a callout line: a coloured marker, then the text.
pub fn format_callout(kind: Callout, text: &str) -> String {
    let marker = match kind {
        Callout::Finding => "✓".color(OK),
        Callout::Warning => "!".color(WARN),
        Callout::Link => "↗".color(LINK),
        Callout::Info => "·".dimmed(),
    };
    format!("{marker} {text}")
}

/// Print [`format_callout`] to stdout.
pub fn print_callout(kind: Callout, text: &str) {
    println!("{}", format_callout(kind, text));
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn no_color() {
        colored::control::set_override(false);
    }

    #[test]
    fn fmt_count_groups_thousands() {
        assert_eq!(fmt_count(0), "0");
        assert_eq!(fmt_count(999), "999");
        assert_eq!(fmt_count(1284), "1,284");
        assert_eq!(fmt_count(1_000_000), "1,000,000");
    }

    #[test]
    fn fmt_compact_picks_suffix() {
        assert_eq!(fmt_compact(42.0), "42");
        assert_eq!(fmt_compact(2.5), "2.5");
        assert_eq!(fmt_compact(1200.0), "1.2k");
        assert_eq!(fmt_compact(1000.0), "1k");
        assert_eq!(fmt_compact(3_400_000.0), "3.4M");
    }

    #[test]
    fn visible_width_ignores_ansi() {
        assert_eq!(visible_width("abc"), 3);
        assert_eq!(visible_width("\x1b[31mabc\x1b[0m"), 3);
        assert_eq!(visible_width("\x1b[1;2mé\x1b[0m"), 1);
    }

    #[test]
    fn header_pushes_right_side_to_margin() {
        no_color();
        let out = format_header(&["svc", "errors"], Some("1,284 errors"), None);
        let mut lines = out.lines();
        let first = lines.next().unwrap();
        assert!(first.starts_with("svc · errors"));
        assert!(first.ends_with("1,284 errors"));
        assert_eq!(visible_width(first), DEFAULT_WIDTH);
        let rule = lines.next().unwrap();
        assert_eq!(rule.chars().count(), DEFAULT_WIDTH);
        assert!(rule.chars().all(|c| c == '─'));
    }

    #[test]
    fn header_without_right_side_has_no_padding() {
        no_color();
        let out = format_header(&["only"], None, None);
        assert_eq!(out.lines().next().unwrap(), "only");
    }

    #[test]
    fn header_badge_is_wrapped_in_spaces() {
        no_color();
        let badge = Badge::new("▲ 9.4×", ERROR);
        let out = format_header(&["svc"], Some("12"), Some(&badge));
        assert!(out.lines().next().unwrap().ends_with("12   ▲ 9.4× "));
    }

    #[test]
    fn numeric_cell_detection() {
        assert_eq!(numeric_cell("42"), Some(true));
        assert_eq!(numeric_cell("1,284"), Some(true));
        assert_eq!(numeric_cell("87.5%"), Some(true));
        assert_eq!(numeric_cell("9.4×"), Some(true));
        assert_eq!(numeric_cell("250ms"), Some(true));
        assert_eq!(numeric_cell("abc"), Some(false));
        assert_eq!(numeric_cell("-"), None);
        assert_eq!(numeric_cell(""), None);
    }

    #[test]
    fn table_has_no_borders_and_uppercase_headers() {
        no_color();
        let out = format_table(
            &["Name".into(), "Count".into()],
            vec![
                vec!["alpha".into(), "5".into()],
                vec!["beta-long".into(), "1,284".into()],
            ],
        );
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 3);
        assert!(!out.contains('|'));
        assert!(!out.contains('+'));
        assert!(lines[0].starts_with("NAME"));
        assert!(lines[0].contains("COUNT"));
        // No trailing whitespace anywhere.
        assert!(lines.iter().all(|l| !l.ends_with(' ')));
    }

    #[test]
    fn table_right_aligns_numeric_columns() {
        no_color();
        let out = format_table(
            &["Name".into(), "Count".into()],
            vec![
                vec!["a".into(), "5".into()],
                vec!["b".into(), "1,284".into()],
            ],
        );
        let lines: Vec<&str> = out.lines().collect();
        // Both numbers end at the same column: "5" sits under the "4".
        assert_eq!(lines[1].len(), lines[2].len());
        assert!(lines[1].ends_with("    5"));
        assert!(lines[2].ends_with("1,284"));
    }

    #[test]
    fn table_keeps_text_columns_left_aligned() {
        no_color();
        let out = format_table(
            &["Name".into(), "Status".into()],
            vec![
                vec!["a".into(), "ok".into()],
                vec!["bbbb".into(), "failed".into()],
            ],
        );
        let lines: Vec<&str> = out.lines().collect();
        assert!(lines[1].starts_with("a     ok"), "{out:?}");
        assert!(lines[2].starts_with("bbbb  failed"), "{out:?}");
    }

    #[test]
    fn sparkline_scales_to_max() {
        assert_eq!(sparkline(&[0.0, 0.0]), "▁▁");
        assert_eq!(sparkline(&[0.0, 6.0, 14.0]), "▁▄█");
        assert_eq!(sparkline(&[f64::NAN, 1.0]), " █");
        assert_eq!(sparkline(&[]), "");
    }

    #[test]
    fn downsample_sums_buckets() {
        assert_eq!(downsample(&[1.0, 2.0, 3.0, 4.0], 2), vec![3.0, 7.0]);
        assert_eq!(downsample(&[1.0, 2.0, 3.0], 2), vec![3.0, 3.0]);
        assert_eq!(downsample(&[1.0, 2.0], 5), vec![1.0, 2.0]);
        assert_eq!(downsample(&[1.0, 2.0], 0), vec![1.0, 2.0]);
    }

    #[test]
    fn bar_chart_has_axes_and_labels() {
        no_color();
        let out = format_bar_chart(
            &[0.0, 40.0, 120.0, 60.0, 0.0, 0.0, 0.0, 0.0],
            &[(0, "13:30".into()), (6, "14:30".into())],
            3,
            1,
            None,
        );
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 5, "{out}");
        assert_eq!(lines[0], "120 ┤  █");
        assert_eq!(lines[1], " 80 ┤  █▄");
        assert_eq!(lines[2], " 40 ┤ ███");
        assert_eq!(lines[3], "  0 └────────");
        assert_eq!(lines[4], "     13:30 14:30");
    }

    #[test]
    fn bar_chart_all_zero_does_not_divide_by_zero() {
        no_color();
        let out = format_bar_chart(&[0.0, 0.0], &[], 2, 1, None);
        assert!(out.contains("└──"));
    }

    #[test]
    fn bar_chart_widens_bars_and_skips_overlapping_labels() {
        no_color();
        let labels: Vec<(usize, String)> = (0..4).map(|i| (i, format!("1{i}:00"))).collect();
        let out = format_bar_chart(&[1.0, 2.0, 3.0, 4.0], &labels, 2, 3, None);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines[0], "4 ┤      ▄▄▄███");
        assert_eq!(lines[1], "2 ┤▄▄▄█████████");
        assert_eq!(lines[2], "0 └────────────");
        // Labels are 5 wide, bars 3 wide: every second label is dropped.
        assert_eq!(lines[3], "   10:00  12:00");
    }

    #[test]
    fn callout_markers() {
        no_color();
        assert_eq!(format_callout(Callout::Finding, "x"), "✓ x");
        assert_eq!(format_callout(Callout::Warning, "x"), "! x");
        assert_eq!(format_callout(Callout::Link, "x"), "↗ x");
        assert_eq!(format_callout(Callout::Info, "x"), "· x");
    }
}
