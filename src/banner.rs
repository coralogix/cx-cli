use std::io::IsTerminal;

use colored::Colorize;
use terminal_size::{terminal_size, Width};

const LOGO_LINES: &[&str] = &[
    r"  ██████╗██╗  ██╗",
    r" ██╔════╝╚██╗██╔╝",
    r" ██║      ╚███╔╝ ",
    r" ██║      ██╔██╗ ",
    r" ╚██████╗██╔╝ ██╗",
    r"  ╚═════╝╚═╝  ╚═╝",
];

const TAGLINE: &str = "The observability backbone for AI agents and engineering teams";

/// The display width of the logo (number of visible characters, not bytes).
/// Each Unicode block char (█, ╗, etc.) is 1 column wide in monospace terminals.
fn display_width(s: &str) -> usize {
    s.chars().count()
}

fn term_width() -> usize {
    terminal_size()
        .map(|(Width(w), _)| w as usize)
        .unwrap_or(80)
}

fn center_pad(content_width: usize, total_width: usize) -> String {
    if content_width >= total_width {
        return String::new();
    }
    " ".repeat((total_width - content_width) / 2)
}

/// The `--help` CX logo, gradient-colored, with no centering, tagline, or
/// separator. Public so other commands (e.g. `cx init`'s success banner) can
/// print the same art left-aligned. Callers add their own leading/trailing
/// blank lines.
pub fn render_logo() -> String {
    let total = LOGO_LINES.len();
    LOGO_LINES
        .iter()
        .enumerate()
        .map(|(i, line)| gradient_line(line, i, total))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn should_show() -> bool {
    std::io::stdout().is_terminal()
        && std::env::var("NO_COLOR").is_err()
        && !crate::safety::is_agent_mode()
}

pub fn render() -> String {
    let width = term_width();
    let logo_width = LOGO_LINES
        .iter()
        .map(|l| display_width(l))
        .max()
        .unwrap_or(0);
    let total = LOGO_LINES.len();

    let mut out: Vec<String> = Vec::with_capacity(total + 6);
    out.push(String::new());

    // ── Centered gradient logo ──
    for (i, line) in LOGO_LINES.iter().enumerate() {
        let pad = center_pad(display_width(line), width);
        out.push(format!("{}{}", pad, gradient_line(line, i, total)));
    }

    out.push(String::new());

    // ── Centered styled tagline ──
    let tag_pad = center_pad(TAGLINE.len(), width);
    out.push(format!(
        "{}{}",
        tag_pad,
        TAGLINE.truecolor(0, 170, 110).italic()
    ));

    // ── Separator line ──
    let sep_width = logo_width.max(TAGLINE.len()) + 4;
    let sep_pad = center_pad(sep_width, width);
    let separator: String = "─".repeat(sep_width);
    out.push(format!("{}{}", sep_pad, separator.truecolor(0, 90, 60)));

    out.join("\n")
}

fn gradient_line(text: &str, row: usize, total_rows: usize) -> String {
    let t = if total_rows <= 1 {
        0.0
    } else {
        row as f64 / (total_rows - 1) as f64
    };
    // Vivid mint (#00FFB0) → bright emerald (#00E078)
    let r = 0;
    let g = (255.0 - 31.0 * t) as u8;
    let b = (176.0 - 56.0 * t) as u8;
    format!("{}", text.truecolor(r, g, b).bold())
}
