//! Small terminal UI helpers: colors, icons, time formatting.

use std::fmt;

pub const DIM: &str = "\x1b[2m";
pub const BOLD: &str = "\x1b[1m";
pub const RED: &str = "\x1b[31m";
pub const GREEN: &str = "\x1b[32m";
pub const YELLOW: &str = "\x1b[33m";
pub const CYAN: &str = "\x1b[36m";
pub const RESET: &str = "\x1b[0m";

pub fn colored(color: &str, text: &str) -> String {
    format!("{color}{text}{RESET}")
}

pub fn ok(text: &str) -> String {
    colored(GREEN, text)
}

pub fn warn(text: &str) -> String {
    colored(YELLOW, text)
}

pub fn err(text: &str) -> String {
    colored(RED, text)
}

pub fn dim(text: &str) -> String {
    colored(DIM, text)
}

pub fn bold(text: &str) -> String {
    colored(BOLD, text)
}

pub fn cyan(text: &str) -> String {
    colored(CYAN, text)
}

/// Icons for workflow status / conclusion.
pub fn run_icon(status: &str, conclusion: Option<String>) -> String {
    match conclusion.as_deref() {
        Some("success") => ok("✓"),
        Some("failure") | Some("timed_out") => err("✗"),
        Some("cancelled") => warn("⊘"),
        Some(_) => warn("?"),
        None => match status {
            "in_progress" | "queued" | "waiting" | "pending" | "requested" => cyan("●"),
            _ => dim("·"),
        },
    }
}

pub fn fmt_ts(ts: i64) -> String {
    use chrono::TimeZone;
    chrono::Utc
        .timestamp_opt(ts, 0)
        .single()
        .map(|t| t.format("%Y-%m-%d %H:%M:%S UTC").to_string())
        .unwrap_or_else(|| ts.to_string())
}

pub fn fmt_duration(secs: i64) -> String {
    if secs < 0 {
        return "—".into();
    }
    let m = secs / 60;
    let s = secs % 60;
    if m > 0 {
        format!("{m}m{s:02}s")
    } else {
        format!("{s}s")
    }
}

/// Truncate for table cells.
pub fn trunc(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let t: String = s.chars().take(n.saturating_sub(1)).collect();
        format!("{t}…")
    }
}

/// Check/cross mark.
pub fn mark(v: bool) -> String {
    if v {
        ok("✓")
    } else {
        err("✗")
    }
}

/// Print a row of cells with fixed widths.
pub struct Table {
    widths: Vec<usize>,
}

impl Table {
    pub fn new(widths: &[usize]) -> Self {
        Self {
            widths: widths.to_vec(),
        }
    }

    pub fn row(&self, cells: &[impl fmt::Display]) {
        let mut line = String::new();
        for (i, c) in cells.iter().enumerate() {
            let w = self.widths.get(i).copied().unwrap_or(20);
            line.push_str(&format!("{:<width$} ", c.to_string(), width = w));
        }
        println!("{}", line.trim_end());
    }
}
