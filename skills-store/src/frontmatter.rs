//! Minimal SKILL.md frontmatter parser, ported from `src/frontmatter.ts`.
//!
//! Only `---` delimited YAML is supported (no `---js` to avoid eval RCE).

use yaml_rust2::{Yaml, YamlLoader};

/// Parsed frontmatter: `data` as JSON-compatible values, `content` the body.
#[derive(Debug, Clone, PartialEq)]
pub struct Frontmatter {
    /// Parsed YAML frontmatter as a JSON value (object, possibly empty).
    pub data: serde_json::Value,
    /// Document body after the closing fence.
    pub content: String,
}

/// Parse `raw` into frontmatter + body. Without a leading `---` fence the
/// whole input is the content and `data` is an empty object.
#[must_use]
pub fn parse_frontmatter(raw: &str) -> Frontmatter {
    let input = raw.strip_prefix('\u{FEFF}').unwrap_or(raw);
    let Some(fence_end) = opening_fence_len(input) else {
        return Frontmatter { data: serde_json::Value::Object(Default::default()), content: raw.to_string() };
    };
    let rest = &input[fence_end..];
    // Find the closing fence: a line that is exactly `---` (allowing a
    // trailing `\r`), terminated by `\n` or end of input.
    let mut close_start = None;
    let mut offset = 0;
    while let Some(rel) = rest[offset..].find("---") {
        let at = offset + rel;
        let line_start_is_boundary = at == 0 || rest[..at].ends_with('\n');
        if line_start_is_boundary {
            let after = &rest[at + 3..];
            let after = after.strip_prefix('\r').unwrap_or(after);
            if after.starts_with('\n') || after.is_empty() {
                close_start = Some(at);
                break;
            }
        }
        offset = at + 3;
    }
    let Some(close_start) = close_start else {
        return Frontmatter { data: serde_json::Value::Object(Default::default()), content: raw.to_string() };
    };
    // Body excludes the newline before the closing fence.
    let mut body = &rest[..close_start];
    if body.ends_with('\n') {
        body = &body[..body.len() - 1];
    }
    if body.ends_with('\r') {
        body = &body[..body.len() - 1];
    }
    // Content starts after the closing fence's line terminator.
    let after = &rest[close_start + 3..];
    let after = after.strip_prefix('\r').unwrap_or(after);
    let content = after.strip_prefix('\n').unwrap_or(after);
    let data = parse_yaml_object(body);
    Frontmatter { data, content: content.to_string() }
}

fn opening_fence_len(input: &str) -> Option<usize> {
    let rest = input.strip_prefix("---")?;
    let rest = rest.strip_prefix('\r').unwrap_or(rest);
    if rest.starts_with('\n') {
        Some(input.len() - rest.len())
    } else {
        None
    }
}

fn parse_yaml_object(body: &str) -> serde_json::Value {
    match YamlLoader::load_from_str(body) {
        Ok(docs) => {
            let value = docs.into_iter().next().unwrap_or(Yaml::Null);
            yaml_to_json(&value)
        }
        Err(_) => serde_json::Value::Object(Default::default()),
    }
}

/// Convert a YAML value into its JSON equivalent (objects get string keys,
/// matching the JS `yaml` package's behavior for SKILL.md frontmatter).
#[must_use]
pub fn yaml_to_json(value: &Yaml) -> serde_json::Value {
    match value {
        Yaml::Null | Yaml::BadValue => serde_json::Value::Null,
        Yaml::Boolean(b) => serde_json::Value::Bool(*b),
        Yaml::Integer(i) => serde_json::json!(i),
        Yaml::Real(s) => s.parse::<f64>().map(|f| serde_json::json!(f)).unwrap_or_else(|_| serde_json::json!(s)),
        Yaml::String(s) => serde_json::json!(s),
        Yaml::Array(items) => {
            serde_json::Value::Array(items.iter().map(yaml_to_json).collect())
        }
        Yaml::Hash(map) => {
            let mut object = serde_json::Map::new();
            for (key, val) in map {
                let key = match key {
                    Yaml::String(s) => s.clone(),
                    Yaml::Integer(i) => i.to_string(),
                    Yaml::Real(s) => s.clone(),
                    Yaml::Boolean(b) => b.to_string(),
                    Yaml::Null => "null".to_string(),
                    other => format!("{other:?}"),
                };
                object.insert(key, yaml_to_json(val));
            }
            serde_json::Value::Object(object)
        }
        other => serde_json::json!(format!("{other:?}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_basic_frontmatter() {
        let raw = "---\nname: my-skill\ndescription: Does things\n---\n\n# Body\n";
        let fm = parse_frontmatter(raw);
        assert_eq!(fm.data["name"], json!("my-skill"));
        assert_eq!(fm.data["description"], json!("Does things"));
        assert_eq!(fm.content, "\n# Body\n");
    }

    #[test]
    fn handles_crlf_and_bom() {
        let raw = "\u{FEFF}---\r\nname: a\r\ndescription: b\r\n---\r\nbody";
        let fm = parse_frontmatter(raw);
        assert_eq!(fm.data["name"], json!("a"));
        assert_eq!(fm.content, "body");
    }

    #[test]
    fn no_fence_returns_raw() {
        let fm = parse_frontmatter("just text");
        assert_eq!(fm.data, json!({}));
        assert_eq!(fm.content, "just text");
    }

    #[test]
    fn nested_metadata() {
        let raw = "---\nname: x\ndescription: y\nmetadata:\n  internal: true\n---\nbody";
        let fm = parse_frontmatter(raw);
        assert_eq!(fm.data["metadata"]["internal"], json!(true));
    }

    #[test]
    fn dashes_in_body_do_not_confuse_parser() {
        let raw = "---\nname: x\ndescription: y\n---\n\n---\n still body";
        let fm = parse_frontmatter(raw);
        assert_eq!(fm.content, "\n---\n still body");
    }
}
