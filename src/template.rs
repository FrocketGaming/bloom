//! Snippet bodies: `{{date}}`, `{{time}}`, `{{clipboard}}`, `{{cursor}}`, `{{field:Label|opt|opt}}`.

use serde::Serialize;
use std::collections::HashMap;
use std::fmt::Write;

#[derive(Debug, Clone, PartialEq)]
pub enum Seg {
    Text(String),
    Var { name: String, arg: Option<String> },
    Field { label: String, options: Vec<String> },
    Cursor,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct FieldSpec {
    pub label: String,
    pub options: Vec<String>,
}

/// Preview pieces, so the UI can highlight what Bloom fills in.
#[derive(Debug, Clone, Serialize)]
pub struct Part {
    pub kind: &'static str, // "text" | "var" | "field" | "cursor"
    pub text: String,
}

pub struct Rendered {
    pub text: String,
    /// How many characters come after the `{{cursor}}` marker.
    pub cursor_back: usize,
}

pub fn parse(body: &str) -> Vec<Seg> {
    let mut segs = Vec::new();
    let mut text = String::new();
    let mut rest = body;
    while let Some(open) = rest.find("{{") {
        let Some(close) = rest[open + 2..].find("}}") else {
            break;
        };
        let inner = rest[open + 2..open + 2 + close].trim();
        let (name, arg) = match inner.split_once(':') {
            Some((n, a)) => (n.trim().to_lowercase(), Some(a.trim().to_string())),
            None => (inner.to_lowercase(), None),
        };
        let seg = match name.as_str() {
            "date" | "time" | "clipboard" => Some(Seg::Var { name, arg }),
            "cursor" => Some(Seg::Cursor),
            "field" => arg.filter(|a| !a.is_empty()).map(|a| {
                let mut parts = a.split('|').map(|p| p.trim().to_string());
                let label = parts.next().unwrap_or_default();
                Seg::Field {
                    label,
                    options: parts.filter(|p| !p.is_empty()).collect(),
                }
            }),
            _ => None,
        };
        text.push_str(&rest[..open]);
        match seg {
            Some(seg) => {
                if !text.is_empty() {
                    segs.push(Seg::Text(std::mem::take(&mut text)));
                }
                segs.push(seg);
            }
            // Unknown placeholders stay as typed.
            None => text.push_str(&rest[open..open + 4 + close]),
        }
        rest = &rest[open + 4 + close..];
    }
    text.push_str(rest);
    if !text.is_empty() {
        segs.push(Seg::Text(text));
    }
    segs
}

pub fn fields(segs: &[Seg]) -> Vec<FieldSpec> {
    let mut out: Vec<FieldSpec> = Vec::new();
    for s in segs {
        if let Seg::Field { label, options } = s {
            if let Some(f) = out.iter_mut().find(|f| f.label == *label) {
                if f.options.is_empty() {
                    f.options = options.clone();
                }
            } else {
                out.push(FieldSpec {
                    label: label.clone(),
                    options: options.clone(),
                });
            }
        }
    }
    out
}

fn fmt_time(fmt: &str) -> String {
    let mut s = String::new();
    // An invalid format string makes chrono return an error rather than text.
    match write!(s, "{}", chrono::Local::now().format(fmt)) {
        Ok(()) => s,
        Err(_) => format!("[bad format: {fmt}]"),
    }
}

pub fn var_value(name: &str, arg: Option<&str>) -> String {
    match name {
        "date" => fmt_time(arg.unwrap_or("%b %-d, %Y")),
        "time" => fmt_time(arg.unwrap_or("%-I:%M %p")),
        "clipboard" => arboard::Clipboard::new()
            .and_then(|mut c| c.get_text())
            .unwrap_or_default(),
        _ => String::new(),
    }
}

pub fn render(segs: &[Seg], values: &HashMap<String, String>) -> Rendered {
    let mut text = String::new();
    let mut cursor_at: Option<usize> = None;
    for s in segs {
        match s {
            Seg::Text(t) => text.push_str(t),
            Seg::Var { name, arg } => text.push_str(&var_value(name, arg.as_deref())),
            Seg::Field { label, .. } => {
                text.push_str(values.get(label).map(String::as_str).unwrap_or(""))
            }
            Seg::Cursor => {
                if cursor_at.is_none() {
                    cursor_at = Some(text.chars().count());
                }
            }
        }
    }
    let total = text.chars().count();
    Rendered {
        cursor_back: cursor_at.map(|c| total - c).unwrap_or(0),
        text,
    }
}

pub fn preview(segs: &[Seg]) -> Vec<Part> {
    segs.iter()
        .map(|s| match s {
            Seg::Text(t) => Part {
                kind: "text",
                text: t.clone(),
            },
            Seg::Var { name, arg } => {
                let mut v = var_value(name, arg.as_deref());
                if v.chars().count() > 120 {
                    v = v.chars().take(120).collect::<String>() + "…";
                }
                if v.is_empty() {
                    v = format!("({name} is empty)");
                }
                Part {
                    kind: "var",
                    text: v,
                }
            }
            Seg::Field { label, .. } => Part {
                kind: "field",
                text: label.clone(),
            },
            Seg::Cursor => Part {
                kind: "cursor",
                text: String::new(),
            },
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_renders() {
        let segs =
            parse("Hi {{field:Name}}, {{cursor}}bye {{field:Tone|A|B}} {{nope}} {{field:Name}}");
        let f = fields(&segs);
        assert_eq!(f.len(), 2);
        assert_eq!(f[1].options, vec!["A", "B"]);
        let mut v = HashMap::new();
        v.insert("Name".to_string(), "Dana".to_string());
        v.insert("Tone".to_string(), "B".to_string());
        let r = render(&segs, &v);
        assert_eq!(r.text, "Hi Dana, bye B {{nope}} Dana");
        assert_eq!(r.cursor_back, "bye B {{nope}} Dana".chars().count());
    }

    #[test]
    fn bad_date_format_does_not_panic() {
        let r = render(&parse("{{date:%Q}}"), &HashMap::new());
        assert!(r.text.starts_with("[bad format"));
    }

    #[test]
    fn unclosed_braces_are_text() {
        assert_eq!(parse("a {{date"), vec![Seg::Text("a {{date".into())]);
    }
}
