//! Snippet bodies: `{{date}}`, `{{time}}`, `{{clipboard}}`, `{{cursor}}`, `{{field:Label|opt|opt}}`.

use crate::variables::{Content, Variables};
use serde::Serialize;
use std::collections::HashMap;
use std::fmt::Write;

#[derive(Debug, Clone, PartialEq)]
pub enum Seg {
    Text(String),
    Var {
        name: String,
        arg: Option<String>,
    },
    Field {
        label: String,
        options: Vec<String>,
    },
    Cursor,
    /// `{{var:name}}` before the variable is looked up. If no variable has
    /// that name it stays as typed, so `raw` is exactly what was written.
    UserVar {
        name: String,
        raw: String,
    },
    /// A text variable, looked up. Inserted exactly as written: placeholders
    /// inside it are not expanded.
    Value {
        name: String,
        text: String,
    },
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
            "var" => arg.filter(|a| !a.is_empty()).map(|name| Seg::UserVar {
                name,
                raw: rest[open..open + 4 + close].to_string(),
            }),
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

/// Looks up every `{{var:name}}`. A text variable becomes its value and a list
/// becomes a pick-one field labelled with the variable's name. A name that no
/// variable has stays as typed.
pub fn resolve(segs: &[Seg], variables: &Variables) -> Vec<Seg> {
    segs.iter()
        .map(|seg| match seg {
            Seg::UserVar { name, .. } => match variables.get(name) {
                Some(variable) => match &variable.content {
                    Content::Text(text) => Seg::Value {
                        name: variable.name.clone(),
                        text: text.clone(),
                    },
                    // The label is the name as defined, so `{{var:Signoff}}` and
                    // `{{var:signoff}}` are asked for once.
                    Content::Choices(options) => Seg::Field {
                        label: variable.name.clone(),
                        options: options.clone(),
                    },
                },
                None => seg.clone(),
            },
            other => other.clone(),
        })
        .collect()
}

/// Parses a body and looks up its variables in one go.
pub fn parse_resolved(body: &str, variables: &Variables) -> Vec<Seg> {
    resolve(&parse(body), variables)
}

/// The names used as `{{var:name}}` that no variable has, in order of first use.
pub fn missing(segs: &[Seg]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for seg in segs {
        if let Seg::UserVar { name, .. } = seg {
            if !out.iter().any(|n| n.eq_ignore_ascii_case(name)) {
                out.push(name.clone());
            }
        }
    }
    out
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

/// Reads the clipboard once, for callers that show several previews at a time.
/// Opening the palette shouldn't open the clipboard once per snippet.
pub fn read_clipboard() -> Option<String> {
    arboard::Clipboard::new()
        .ok()
        .and_then(|mut c| c.get().text().ok())
}

/// True when a body needs the clipboard, so callers can avoid reading it.
pub fn uses_clipboard(segs: &[Seg]) -> bool {
    segs.iter()
        .any(|s| matches!(s, Seg::Var { name, .. } if name == "clipboard"))
}

pub fn var_value(name: &str, arg: Option<&str>) -> String {
    match name {
        "date" => fmt_time(arg.unwrap_or("%b %-d, %Y")),
        "time" => fmt_time(arg.unwrap_or("%-I:%M %p")),
        "clipboard" => read_clipboard().unwrap_or_default(),
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
            Seg::Value { text: value, .. } => text.push_str(value),
            Seg::UserVar { raw, .. } => text.push_str(raw),
        }
    }
    let total = text.chars().count();
    Rendered {
        cursor_back: cursor_at.map(|c| total - c).unwrap_or(0),
        text,
    }
}

pub fn preview(segs: &[Seg], clipboard: Option<&str>) -> Vec<Part> {
    segs.iter()
        .map(|s| match s {
            Seg::Text(t) => Part {
                kind: "text",
                text: t.clone(),
            },
            Seg::Var { name, arg } => {
                let mut v = if name == "clipboard" {
                    clipboard.unwrap_or_default().to_string()
                } else {
                    var_value(name, arg.as_deref())
                };
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
            Seg::Value { name, text } => Part {
                kind: "var",
                text: if text.is_empty() {
                    format!("({name} is empty)")
                } else if text.chars().count() > 120 {
                    text.chars().take(120).collect::<String>() + "…"
                } else {
                    text.clone()
                },
            },
            Seg::UserVar { raw, .. } => Part {
                kind: "missing",
                text: raw.clone(),
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

    fn variables() -> Variables {
        use crate::variables::Variable;
        Variables {
            list: vec![
                Variable {
                    name: "Email".into(),
                    content: Content::Text("sam@example.com".into()),
                },
                Variable {
                    name: "address".into(),
                    content: Content::Text("1 Main St\nTown".into()),
                },
                Variable {
                    name: "note".into(),
                    content: Content::Text("send {{date}} and {{field:Name}}".into()),
                },
                Variable {
                    name: "signoff".into(),
                    content: Content::Choices(vec!["Best".into(), "Cheers".into()]),
                },
            ],
            errors: Vec::new(),
        }
    }

    fn render_with(body: &str, values: &[(&str, &str)]) -> String {
        let segs = parse_resolved(body, &variables());
        let values = values
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        render(&segs, &values).text
    }

    #[test]
    fn a_text_variable_is_inserted_as_written() {
        assert_eq!(
            render_with("Mail {{var:email}}.", &[]),
            "Mail sam@example.com."
        );
        assert_eq!(render_with("{{var:address}}", &[]), "1 Main St\nTown");
    }

    #[test]
    fn variable_names_ignore_case_and_spacing() {
        let expected = "sam@example.com";
        assert_eq!(render_with("{{var:EMAIL}}", &[]), expected);
        assert_eq!(render_with("{{ var : email }}", &[]), expected);
        assert_eq!(render_with("{{VAR:Email}}", &[]), expected);
    }

    #[test]
    fn placeholders_inside_a_text_variable_are_not_expanded() {
        let segs = parse_resolved("{{var:note}}", &variables());
        assert!(fields(&segs).is_empty(), "no field is created from a value");
        assert_eq!(
            render(&segs, &HashMap::new()).text,
            "send {{date}} and {{field:Name}}"
        );
    }

    #[test]
    fn a_list_variable_becomes_one_pick_one_field() {
        let segs = parse_resolved("{{var:signoff}} / {{var:SignOff}}", &variables());
        let f = fields(&segs);
        assert_eq!(f.len(), 1, "asked for once, however it is spelled");
        assert_eq!(f[0].label, "signoff");
        assert_eq!(f[0].options, vec!["Best", "Cheers"]);
        assert_eq!(
            render_with("{{var:signoff}}!", &[("signoff", "Cheers")]),
            "Cheers!"
        );
    }

    #[test]
    fn an_undefined_variable_stays_as_typed_and_is_reported() {
        let segs = parse_resolved(
            "Hi {{var:Emial}} and {{var:emial}} and {{var:email}}",
            &variables(),
        );
        assert_eq!(
            render(&segs, &HashMap::new()).text,
            "Hi {{var:Emial}} and {{var:emial}} and sam@example.com"
        );
        assert_eq!(
            missing(&segs),
            vec!["Emial"],
            "reported once, ignoring case"
        );
        assert!(missing(&parse_resolved("{{var:email}}", &variables())).is_empty());
    }

    #[test]
    fn an_empty_variable_name_is_plain_text() {
        assert_eq!(render_with("a {{var:}} b", &[]), "a {{var:}} b");
        assert!(missing(&parse("{{var:}}")).is_empty());
    }

    #[test]
    fn the_cursor_marker_counts_variable_text() {
        let segs = parse_resolved("{{var:address}}{{cursor}}!", &variables());
        assert_eq!(render(&segs, &HashMap::new()).cursor_back, 1);
        let segs = parse_resolved("{{cursor}}{{var:address}}", &variables());
        assert_eq!(
            render(&segs, &HashMap::new()).cursor_back,
            "1 Main St\nTown".chars().count()
        );
    }

    #[test]
    fn preview_marks_values_fields_and_missing_names() {
        let segs = parse_resolved("{{var:email}} {{var:signoff}} {{var:nope}}", &variables());
        let parts = preview(&segs, None);
        assert_eq!(
            (parts[0].kind, parts[0].text.as_str()),
            ("var", "sam@example.com")
        );
        assert_eq!(
            (parts[2].kind, parts[2].text.as_str()),
            ("field", "signoff")
        );
        assert_eq!(
            (parts[4].kind, parts[4].text.as_str()),
            ("missing", "{{var:nope}}")
        );
    }

    #[test]
    fn without_a_lookup_a_variable_renders_as_typed() {
        let segs = parse("{{var:email}}");
        assert_eq!(render(&segs, &HashMap::new()).text, "{{var:email}}");
    }

    #[test]
    fn preview_takes_the_clipboard_it_is_given() {
        let segs = parse("[{{clipboard}}]");
        assert!(uses_clipboard(&segs));
        assert_eq!(preview(&segs, Some("hi"))[1].text, "hi");
        assert_eq!(preview(&segs, None)[1].text, "(clipboard is empty)");
        assert!(!uses_clipboard(&parse("{{date}}")));
    }
}
