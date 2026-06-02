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
        TAGLINE.truecolor(60, 140, 220).italic()
    ));

    // ── Separator line ──
    let sep_width = logo_width.max(TAGLINE.len()) + 4;
    let sep_pad = center_pad(sep_width, width);
    let separator: String = "─".repeat(sep_width);
    out.push(format!("{}{}", sep_pad, separator.truecolor(20, 60, 140)));

    out.join("\n")
}

fn gradient_line(text: &str, row: usize, total_rows: usize) -> String {
    let t = if total_rows <= 1 {
        0.0
    } else {
        row as f64 / (total_rows - 1) as f64
    };
    // Bright sky blue (#4FC3FF) → deep royal blue (#1E5BCC)
    let r = (79.0 - 49.0 * t) as u8;
    let g = (195.0 - 104.0 * t) as u8;
    let b = (255.0 - 51.0 * t) as u8;
    format!("{}", text.truecolor(r, g, b).bold())
}
