//! Cheap, line-based, token-scan syntax highlighting (no regex crate).
//!
//! [`highlight`] turns one line into colored [`Span`]s and carries
//! multi-line state (block comments / strings / code fences) in [`HlState`].
//! [`precompute`] walks a whole file once so virtualized rendering can
//! re-use the result.

use egui::Color32;

use crate::theme;

const KEYWORD: Color32 = Color32::from_rgb(0x56, 0x9c, 0xd6); // blue
const TYPE: Color32 = Color32::from_rgb(0x4e, 0xc9, 0xb0); // VSCode green
const STRING: Color32 = Color32::from_rgb(0xce, 0x91, 0x78); // orange
const NUMBER: Color32 = Color32::from_rgb(0xb5, 0xce, 0xa8); // light green
const COMMENT: Color32 = Color32::from_rgb(0x6a, 0x99, 0x55); // VSCode green

/// Language mode detected from a path extension.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    Rust,
    Toml,
    Markdown,
    Js,
    Yaml,
    Plain,
}

impl Lang {
    pub fn from_path(path: &str) -> Lang {
        let ext = std::path::Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        match ext.as_str() {
            "rs" => Lang::Rust,
            "toml" => Lang::Toml,
            "md" | "markdown" => Lang::Markdown,
            "js" | "jsx" | "mjs" | "cjs" | "ts" | "tsx" | "mts" | "cts" | "json" => Lang::Js,
            "yml" | "yaml" => Lang::Yaml,
            _ => Lang::Plain,
        }
    }

    /// Single-letter file glyph used in the explorer tree.
    pub fn glyph(&self) -> (&'static str, Color32) {
        match self {
            Lang::Rust => ("R", Color32::from_rgb(0xe4, 0x8b, 0x3d)),
            Lang::Toml => ("T", KEYWORD),
            Lang::Markdown => ("M", KEYWORD),
            Lang::Js => ("J", theme::YELLOW),
            Lang::Yaml => ("Y", theme::RED),
            Lang::Plain => ("·", theme::DIM),
        }
    }
}

/// One colored run of text.
#[derive(Clone, Debug)]
pub struct Span {
    pub text: String,
    pub color: Color32,
}

/// Multi-line carry-over state for block comments / strings / fences.
#[derive(Default, Clone, Debug)]
pub struct HlState {
    pub block_comment: bool,
    pub string_span: bool,
    pub fence: bool,
}

const RUST_KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern",
    "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub",
    "ref", "return", "self", "Self", "static", "struct", "super", "trait", "true", "type",
    "unsafe", "use", "where", "while",
];

const JS_KEYWORDS: &[&str] = &[
    "async",
    "await",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "default",
    "delete",
    "do",
    "else",
    "enum",
    "export",
    "extends",
    "false",
    "finally",
    "for",
    "from",
    "function",
    "if",
    "implements",
    "import",
    "in",
    "instanceof",
    "interface",
    "let",
    "new",
    "null",
    "private",
    "protected",
    "public",
    "readonly",
    "return",
    "static",
    "super",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "type",
    "typeof",
    "undefined",
    "var",
    "void",
    "while",
    "yield",
];

fn is_word_start(c: char) -> bool {
    c.is_alphabetic() || c == '_'
}

fn is_word(c: char) -> bool {
    is_word_start(c) || c.is_ascii_digit()
}

fn flush(out: &mut Vec<Span>, buf: &mut String, color: Color32) {
    if !buf.is_empty() {
        out.push(Span {
            text: std::mem::take(buf),
            color,
        });
    }
}

fn word_color(word: &str, keywords: &[&str]) -> Color32 {
    if keywords.contains(&word) {
        KEYWORD
    } else if word
        .chars()
        .next()
        .map(|c| c.is_uppercase())
        .unwrap_or(false)
    {
        TYPE
    } else {
        theme::TEXT
    }
}

/// Highlight a single line, carrying `state` across lines.
pub fn highlight(lang: Lang, line: &str, st: &mut HlState) -> Vec<Span> {
    match lang {
        Lang::Rust => hl_c_like(line, st, RUST_KEYWORDS),
        Lang::Js => hl_c_like(line, st, JS_KEYWORDS),
        Lang::Toml => hl_toml(line),
        Lang::Yaml => hl_yaml(line),
        Lang::Markdown => hl_markdown(line, st),
        Lang::Plain => vec![Span {
            text: line.to_string(),
            color: theme::TEXT,
        }],
    }
}

fn hl_c_like(line: &str, st: &mut HlState, keywords: &[&str]) -> Vec<Span> {
    let mut out: Vec<Span> = Vec::new();
    let mut plain = String::new();
    let mut acc = String::new(); // string or comment accumulator
    let chars: Vec<char> = line.chars().collect();
    let n = chars.len();
    let mut i = 0usize;

    while i < n {
        if st.block_comment {
            if chars[i] == '*' && i + 1 < n && chars[i + 1] == '/' {
                acc.push_str("*/");
                i += 2;
                st.block_comment = false;
                flush(&mut out, &mut acc, COMMENT);
            } else {
                acc.push(chars[i]);
                i += 1;
            }
            continue;
        }
        if st.string_span {
            if chars[i] == '\\' && i + 1 < n {
                acc.push(chars[i]);
                acc.push(chars[i + 1]);
                i += 2;
                continue;
            }
            acc.push(chars[i]);
            if chars[i] == '`' {
                st.string_span = false;
                flush(&mut out, &mut acc, STRING);
            }
            i += 1;
            continue;
        }
        let c = chars[i];
        if c == '/' && i + 1 < n && chars[i + 1] == '/' {
            flush(&mut out, &mut plain, theme::TEXT);
            let rest: String = chars[i..].iter().collect();
            out.push(Span {
                text: rest,
                color: COMMENT,
            });
            return out;
        }
        if c == '/' && i + 1 < n && chars[i + 1] == '*' {
            flush(&mut out, &mut plain, theme::TEXT);
            acc.push_str("/*");
            i += 2;
            st.block_comment = true;
            continue;
        }
        if c == '"' {
            flush(&mut out, &mut plain, theme::TEXT);
            // single-line double-quoted string
            acc.push('"');
            i += 1;
            while i < n {
                if chars[i] == '\\' && i + 1 < n {
                    acc.push(chars[i]);
                    acc.push(chars[i + 1]);
                    i += 2;
                    continue;
                }
                acc.push(chars[i]);
                if chars[i] == '"' {
                    i += 1;
                    break;
                }
                i += 1;
            }
            flush(&mut out, &mut acc, STRING);
            continue;
        }
        if c == '`' {
            flush(&mut out, &mut plain, theme::TEXT);
            acc.push('`');
            i += 1;
            st.string_span = true; // template literal may span lines
            continue;
        }
        if c.is_ascii_digit() {
            flush(&mut out, &mut plain, theme::TEXT);
            while i < n && (chars[i].is_ascii_hexdigit() || chars[i] == '.' || chars[i] == '_') {
                acc.push(chars[i]);
                i += 1;
            }
            flush(&mut out, &mut acc, NUMBER);
            continue;
        }
        if is_word_start(c) {
            flush(&mut out, &mut plain, theme::TEXT);
            let start = i;
            while i < n && is_word(chars[i]) {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            let color = word_color(&word, keywords);
            out.push(Span { text: word, color });
            continue;
        }
        plain.push(c);
        i += 1;
    }
    if st.block_comment {
        flush(&mut out, &mut acc, COMMENT);
    } else if st.string_span {
        flush(&mut out, &mut acc, STRING);
    }
    flush(&mut out, &mut plain, theme::TEXT);
    out
}

fn hl_toml(line: &str) -> Vec<Span> {
    let t = line.trim_start();
    if t.starts_with('#') {
        return vec![Span {
            text: line.to_string(),
            color: COMMENT,
        }];
    }
    if t.starts_with('[') {
        return vec![Span {
            text: line.to_string(),
            color: KEYWORD,
        }];
    }
    if let Some(eq) = line.find('=') {
        let mut out = vec![
            Span {
                text: line[..eq].to_string(),
                color: theme::YELLOW,
            },
            Span {
                text: "=".to_string(),
                color: theme::TEXT,
            },
        ];
        out.extend(hl_value(&line[eq + 1..]));
        return out;
    }
    vec![Span {
        text: line.to_string(),
        color: theme::TEXT,
    }]
}

fn hl_yaml(line: &str) -> Vec<Span> {
    let t = line.trim_start();
    if t.starts_with('#') {
        return vec![Span {
            text: line.to_string(),
            color: COMMENT,
        }];
    }
    let indent = line.len() - line.trim_start().len();
    let body = &line[indent..];
    let mut out: Vec<Span> = Vec::new();
    if indent > 0 {
        out.push(Span {
            text: line[..indent].to_string(),
            color: theme::TEXT,
        });
    }
    if let Some(rest) = body.strip_prefix("- ") {
        out.push(Span {
            text: "- ".to_string(),
            color: KEYWORD,
        });
        out.extend(hl_value(rest));
        return out;
    }
    if let Some(colon) = body.find(": ") {
        out.push(Span {
            text: body[..colon].to_string(),
            color: theme::YELLOW,
        });
        out.push(Span {
            text: ": ".to_string(),
            color: theme::TEXT,
        });
        out.extend(hl_value(&body[colon + 2..]));
        return out;
    }
    if body.ends_with(':') {
        out.push(Span {
            text: body.to_string(),
            color: theme::YELLOW,
        });
        return out;
    }
    out.extend(hl_value(body));
    out
}

/// Shared scalar scanner for TOML/YAML values (strings, bools, numbers, comments).
fn hl_value(s: &str) -> Vec<Span> {
    let mut out: Vec<Span> = Vec::new();
    let mut plain = String::new();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0usize;
    while i < chars.len() {
        let c = chars[i];
        if c == '#' {
            flush(&mut out, &mut plain, theme::TEXT);
            let rest: String = chars[i..].iter().collect();
            out.push(Span {
                text: rest,
                color: COMMENT,
            });
            return out;
        }
        if c == '"' || c == '\'' {
            flush(&mut out, &mut plain, theme::TEXT);
            let quote = c;
            let mut acc = String::new();
            acc.push(c);
            i += 1;
            while i < chars.len() {
                acc.push(chars[i]);
                if chars[i] == quote {
                    i += 1;
                    break;
                }
                i += 1;
            }
            out.push(Span {
                text: acc,
                color: STRING,
            });
            continue;
        }
        if is_word_start(c) {
            flush(&mut out, &mut plain, theme::TEXT);
            let start = i;
            while i < chars.len() && is_word(chars[i]) {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            let color = match word.as_str() {
                "true" | "false" | "null" | "inf" | "nan" => KEYWORD,
                _ => {
                    if word
                        .chars()
                        .next()
                        .map(|c| c.is_ascii_digit())
                        .unwrap_or(false)
                    {
                        NUMBER
                    } else {
                        theme::TEXT
                    }
                }
            };
            out.push(Span { text: word, color });
            continue;
        }
        if c.is_ascii_digit() {
            flush(&mut out, &mut plain, theme::TEXT);
            while i < chars.len()
                && (chars[i].is_ascii_digit() || chars[i] == '.' || chars[i] == '_')
            {
                plain.push(chars[i]);
                i += 1;
            }
            flush(&mut out, &mut plain, NUMBER);
            continue;
        }
        plain.push(c);
        i += 1;
    }
    flush(&mut out, &mut plain, theme::TEXT);
    out
}

fn hl_markdown(line: &str, st: &mut HlState) -> Vec<Span> {
    let t = line.trim_start();
    if t.starts_with("```") {
        st.fence = !st.fence;
        return vec![Span {
            text: line.to_string(),
            color: theme::DIM,
        }];
    }
    if st.fence {
        return vec![Span {
            text: line.to_string(),
            color: TYPE,
        }];
    }
    if t.starts_with('#') {
        return vec![Span {
            text: line.to_string(),
            color: KEYWORD,
        }];
    }
    if t.starts_with('>') {
        return vec![Span {
            text: line.to_string(),
            color: theme::DIM,
        }];
    }
    // inline pass: `code` spans, otherwise plain
    let mut out: Vec<Span> = Vec::new();
    let mut plain = String::new();
    let mut in_code = false;
    let mut acc = String::new();
    for c in line.chars() {
        if c == '`' {
            if in_code {
                acc.push('`');
                out.push(Span {
                    text: std::mem::take(&mut acc),
                    color: TYPE,
                });
                in_code = false;
            } else {
                flush(&mut out, &mut plain, theme::TEXT);
                acc.push('`');
                in_code = true;
            }
            continue;
        }
        if in_code {
            acc.push(c);
        } else {
            plain.push(c);
        }
    }
    if in_code {
        acc.push('`');
    }
    flush(&mut out, &mut plain, theme::TEXT);
    if !acc.is_empty() {
        out.push(Span {
            text: acc,
            color: TYPE,
        });
    }
    if out.is_empty() {
        out.push(Span {
            text: line.to_string(),
            color: theme::TEXT,
        });
    }
    out
}

/// Highlight a whole document once (call when the active tab changes, then
/// feed the result to the virtualized code view every frame).
pub fn precompute(lang: Lang, content: &str) -> Vec<Vec<Span>> {
    let mut st = HlState::default();
    let mut out: Vec<Vec<Span>> = Vec::new();
    for line in content.lines() {
        out.push(highlight(lang, line, &mut st));
    }
    if out.is_empty() {
        out.push(vec![Span {
            text: String::new(),
            color: theme::TEXT,
        }]);
    }
    out
}
