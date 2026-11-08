//! Parse and validate `agent.config.toml` package manifests.
//!
//! The parser is deliberately strict at the top level (unknown sections are
//! fatal, because a typo'd section silently not installing is the classic
//! footgun) and per-entry tolerant (one bad entry is reported as a
//! diagnostic and skipped; the rest of the manifest still applies). This
//! mirrors the granular failure boundaries Agent Plugins 1.0.0 standardized.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::diag::{codes, CliResult, Diag};

/// The manifest filename the `agentconfig` CLI looks for in a source tree.
pub const MANIFEST_FILE: &str = "agent.config.toml";

/// A parsed `agent.config.toml`.
#[derive(Debug, Clone)]
pub struct Manifest {
    /// `[package]` metadata; `name` becomes the ledger owner tag.
    pub package: PackageInfo,
    /// `[[hooks]]` entries.
    pub hooks: Vec<HookEntry>,
    /// `[[instructions]]` entries (prompt rules for non-hook harnesses).
    pub instructions: Vec<InstructionEntry>,
    /// `[[mcp]]` entries.
    pub mcp: Vec<McpEntry>,
    /// `[[skills]]` entries.
    pub skills: Vec<SkillEntry>,
}

/// `[package]` section.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageInfo {
    /// Package identity, lowercase kebab-case. Also used as the ownership
    /// ledger owner tag for every surface this package installs.
    pub name: String,
    /// Optional semver-ish version string.
    #[serde(default)]
    pub version: Option<String>,
    /// Optional human description.
    #[serde(default)]
    pub description: Option<String>,
}

/// Hook command: an argv array (`["npx", "-y", "pkg"]`) or a raw shell string.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum HookCommandSpec {
    /// Program plus arguments; rendered with POSIX shell quoting by the
    /// library when the harness only accepts strings.
    Program(Vec<String>),
    /// Trusted raw shell syntax (`HookCommand::ShellUnchecked`).
    Shell(String),
}

/// Hook matcher value. A plain string maps to `All` (`"*"`), `Bash`
/// (`"bash"`/`"shell"`, case-insensitive), `Regex` (contains regex
/// metacharacters), or `Exact` (anything else). An array maps to `AnyOf`; an
/// explicit table picks the variant by key.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum MatcherSpec {
    /// Any of these tool names.
    List(Vec<String>),
    /// `{ exact = ... }`, `{ regex = ... }`, or `{ any = [...] }`.
    Explicit(ExplicitMatcher),
    /// Plain string form.
    Plain(String),
}

/// Explicit matcher table form.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExplicitMatcher {
    /// Match exactly this tool name.
    #[serde(default)]
    pub exact: Option<String>,
    /// Pass this regex through to the harness.
    #[serde(default)]
    pub regex: Option<String>,
    /// Match any of these tool names.
    #[serde(default)]
    pub any: Option<Vec<String>>,
}

/// `[[hooks]]` entry.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HookEntry {
    /// Hook tag (the hook owner namespace on disk).
    pub tag: String,
    /// Command to run.
    pub command: HookCommandSpec,
    /// Lifecycle event name (canonical `Event::as_str` spelling, or a custom
    /// event passed through verbatim). Defaults to `PreToolUse`.
    #[serde(default)]
    pub event: Option<String>,
    /// Tool-call matcher. Defaults to `all`.
    #[serde(default)]
    pub matcher: Option<MatcherSpec>,
    /// Timeout in seconds.
    #[serde(default)]
    pub timeout: Option<u64>,
    /// Status message shown while the hook runs.
    #[serde(default)]
    pub status_message: Option<String>,
    /// Inline markdown injected into the harness's rules/memory file
    /// alongside the hook.
    #[serde(default)]
    pub rules: Option<String>,
    /// Path (relative to the manifest) of a markdown file to inject as
    /// `rules`. Mutually exclusive with `rules`.
    #[serde(default)]
    pub rules_file: Option<String>,
    /// Restrict to these agent ids.
    #[serde(default)]
    pub agents: Option<Vec<String>>,
    /// Exclude these agent ids.
    #[serde(default)]
    pub skip: Vec<String>,
}

/// `[[instructions]]` entry.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstructionEntry {
    /// Instruction name; filename stem and fence tag.
    pub name: String,
    /// Inline markdown body. Mutually exclusive with `file`.
    #[serde(default)]
    pub body: Option<String>,
    /// Markdown file path relative to the manifest. Mutually exclusive with
    /// `body`.
    #[serde(default)]
    pub file: Option<String>,
    /// `auto` (default: pick the first placement the target agent supports),
    /// `inline`, `referenced`, or `standalone`.
    #[serde(default)]
    pub placement: Option<String>,
    /// Restrict to these agent ids.
    #[serde(default)]
    pub agents: Option<Vec<String>>,
    /// Exclude these agent ids.
    #[serde(default)]
    pub skip: Vec<String>,
}

/// `[[mcp]]` entry.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McpEntry {
    /// Server name (the key the harness uses).
    pub name: String,
    /// `stdio` (default), `http`, or `sse`.
    #[serde(rename = "type", default)]
    pub kind: Option<String>,
    /// stdio launcher: `[command, args...]`.
    #[serde(default)]
    pub command: Option<Vec<String>>,
    /// Remote URL (http/sse).
    #[serde(default)]
    pub url: Option<String>,
    /// stdio environment overrides. Placeholder values like `${VAR}` are
    /// allowed; literal secrets are refused by the library's secret policy.
    #[serde(default)]
    pub env: Option<BTreeMap<String, String>>,
    /// http/sse headers.
    #[serde(default)]
    pub headers: Option<BTreeMap<String, String>>,
    /// Human-friendly display name.
    #[serde(default)]
    pub friendly_name: Option<String>,
    /// Restrict to these agent ids.
    #[serde(default)]
    pub agents: Option<Vec<String>>,
    /// Exclude these agent ids.
    #[serde(default)]
    pub skip: Vec<String>,
}

/// `[[skills]]` entry.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillEntry {
    /// Directory (relative to the manifest) containing `SKILL.md`; directory
    /// contents become assets.
    #[serde(default)]
    pub path: Option<String>,
    /// Inline skill name (required for the inline form).
    #[serde(default)]
    pub name: Option<String>,
    /// Inline skill description (required for the inline form).
    #[serde(default)]
    pub description: Option<String>,
    /// Inline skill markdown body (required for the inline form).
    #[serde(default)]
    pub body: Option<String>,
    /// Restrict to these agent ids.
    #[serde(default)]
    pub agents: Option<Vec<String>>,
    /// Exclude these agent ids.
    #[serde(default)]
    pub skip: Vec<String>,
}

/// Instruction placement spellings accepted in the manifest.
pub const PLACEMENTS: [&str; 4] = ["auto", "inline", "referenced", "standalone"];

/// Validate a package name: lowercase kebab-case, also a valid owner tag.
pub fn validate_package_name(name: &str) -> CliResult<()> {
    let ok = !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !name.starts_with('-')
        && !name.ends_with('-')
        && !name.contains("--");
    if ok {
        Ok(())
    } else {
        Err(Diag::with_fix(
            codes::INVALID_MANIFEST,
            format!("package name {name:?} must be lowercase kebab-case ([a-z0-9]+(-[a-z0-9]+)*)"),
            "rename the package in [package] name = \"...\"",
        ))
    }
}

/// Locate the manifest in a source directory, returning its path.
pub fn find_manifest(dir: &Path) -> Option<PathBuf> {
    let candidate = dir.join(MANIFEST_FILE);
    candidate.is_file().then_some(candidate)
}

/// Parse a manifest from its TOML text. Returns the manifest plus
/// entry-level diagnostics (invalid entries are skipped, not fatal).
pub fn parse(text: &str) -> CliResult<(Manifest, Vec<Diag>)> {
    let table: toml::Table = text.parse().map_err(|e| {
        Diag::with_fix(
            codes::INVALID_MANIFEST,
            format!("{e}"),
            "fix the TOML syntax in agent.config.toml",
        )
    })?;

    const SECTIONS: [&str; 5] = ["package", "hooks", "instructions", "mcp", "skills"];
    let unknown: Vec<String> = table
        .keys()
        .filter(|k| !SECTIONS.contains(&k.as_str()))
        .cloned()
        .collect();
    if !unknown.is_empty() {
        return Err(Diag::with_fix(
            codes::INVALID_MANIFEST,
            format!("unknown manifest section(s): {}", unknown.join(", ")),
            format!("allowed sections: {}", SECTIONS.join(", ")),
        ));
    }

    let package: PackageInfo = entry(&table, "package", None)?.ok_or_else(|| {
        Diag::with_fix(
            codes::INVALID_MANIFEST,
            "missing [package] section",
            "add [package] with a `name` field",
        )
    })?;
    validate_package_name(&package.name)?;

    let mut diags = Vec::new();
    let hooks = parse_entries(&table, "hooks", &mut diags);
    let instructions = parse_entries(&table, "instructions", &mut diags);
    let mcp = parse_entries(&table, "mcp", &mut diags);
    let skills = parse_entries(&table, "skills", &mut diags);

    Ok((
        Manifest {
            package,
            hooks,
            instructions,
            mcp,
            skills,
        },
        diags,
    ))
}

/// Parse a manifest from a file on disk.
pub fn load(path: &Path) -> CliResult<(Manifest, Vec<Diag>)> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| Diag::new(codes::IO, format!("cannot read {}: {e}", path.display())))?;
    parse(&text)
}

fn entry<T: for<'de> Deserialize<'de>>(
    table: &toml::Table,
    section: &str,
    index: Option<usize>,
) -> CliResult<Option<T>> {
    let Some(value) = table.get(section) else {
        return Ok(None);
    };
    let value = match index {
        Some(i) => {
            let Some(arr) = value.as_array() else {
                return Err(Diag::new(
                    codes::INVALID_MANIFEST,
                    format!("[[{section}]] must be an array of tables"),
                ));
            };
            arr[i].clone()
        }
        None => value.clone(),
    };
    T::deserialize(value)
        .map(Some)
        .map_err(|e| Diag::new(codes::INVALID_ENTRY, format!("{section}: {e}")))
}

fn parse_entries<T: for<'de> Deserialize<'de>>(
    table: &toml::Table,
    section: &str,
    diags: &mut Vec<Diag>,
) -> Vec<T> {
    let Some(value) = table.get(section) else {
        return Vec::new();
    };
    let Some(arr) = value.as_array() else {
        diags.push(Diag::new(
            codes::INVALID_MANIFEST,
            format!("[[{section}]] must be an array of tables; entry ignored"),
        ));
        return Vec::new();
    };
    let mut out = Vec::new();
    for (i, item) in arr.iter().enumerate() {
        match T::deserialize(item.clone()) {
            Ok(entry) => out.push(entry),
            Err(e) => diags.push(Diag::with_fix(
                codes::INVALID_ENTRY,
                format!("{section}[{i}]: {e}"),
                format!("fix or remove {section}[{i}] in agent.config.toml"),
            )),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
[package]
name = "acme-toolkit"
version = "0.1.0"

[[hooks]]
tag = "guard-secrets"
command = ["scripts/guard.sh", "--strict"]
event = "PreToolUse"
matcher = "Bash"
timeout = 30
rules_file = "rules/guard.md"

[[instructions]]
name = "code-style"
file = "STYLE.md"

[[mcp]]
name = "context7"
command = ["npx", "-y", "@upstash/context7-mcp"]
env = { API_KEY = "${CONTEXT7_API_KEY}" }

[[mcp]]
name = "remote"
type = "http"
url = "https://mcp.example.com/mcp"

[[skills]]
path = "skills/pdf-tools"
"#;

    #[test]
    fn parses_sample_manifest() {
        let (m, diags) = parse(SAMPLE).expect("valid manifest");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        assert_eq!(m.package.name, "acme-toolkit");
        assert_eq!(m.hooks.len(), 1);
        assert_eq!(m.instructions.len(), 1);
        assert_eq!(m.mcp.len(), 2);
        assert_eq!(m.skills.len(), 1);
        assert_eq!(m.hooks[0].tag, "guard-secrets");
        assert_eq!(m.mcp[1].kind.as_deref(), Some("http"));
    }

    #[test]
    fn rejects_unknown_top_level_section() {
        let text = format!("{SAMPLE}\n[hookss]\ntag = \"x\"\n");
        let err = parse(&text).unwrap_err();
        assert_eq!(err.code, codes::INVALID_MANIFEST);
        assert!(err.why.contains("hookss"), "why: {}", err.why);
    }

    #[test]
    fn rejects_unknown_entry_key() {
        let text = r#"
[package]
name = "pkg"

[[hooks]]
tag = "t"
command = ["run"]
matchers = "Bash"
"#;
        let (m, diags) = parse(text).expect("structural parse ok");
        assert!(m.hooks.is_empty());
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, codes::INVALID_ENTRY);
        assert!(diags[0].why.contains("hooks[0]"), "why: {}", diags[0].why);
    }

    #[test]
    fn bad_entry_does_not_kill_rest_of_section() {
        let text = r#"
[package]
name = "pkg"

[[mcp]]
name = "broken"
type = "stdio"

[[mcp]]
name = "fine"
command = ["npx", "svc"]
"#;
        let (m, diags) = parse(text).expect("structural parse ok");
        // "stdio" with no `command` key: the entry struct itself parses; the
        // missing command is caught at mapping time, so both survive here.
        assert_eq!(m.mcp.len(), 2);
        assert!(diags.is_empty());
    }

    #[test]
    fn package_name_must_be_kebab() {
        let text = "[package]\nname = \"My Toolkit\"\n";
        let err = parse(text).unwrap_err();
        assert!(err.why.contains("kebab"));
    }

    #[test]
    fn matcher_and_command_forms_parse() {
        let text = r#"
[package]
name = "pkg"

[[hooks]]
tag = "a"
command = "make lint"
matcher = { regex = "Write|Edit" }

[[hooks]]
tag = "b"
command = ["run", "--now"]
matcher = ["Edit", "Write"]

[[hooks]]
tag = "c"
command = ["run"]
matcher = "*"
"#;
        let (m, diags) = parse(text).expect("ok");
        assert!(diags.is_empty());
        assert!(matches!(m.hooks[0].command, HookCommandSpec::Shell(_)));
        assert!(matches!(m.hooks[1].matcher, Some(MatcherSpec::List(_))));
        assert_eq!(m.hooks[2].matcher, Some(MatcherSpec::Plain("*".into())));
    }
}
